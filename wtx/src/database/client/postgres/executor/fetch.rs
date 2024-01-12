use crate::{
  database::{
    client::postgres::{
      executor::commons::FetchWithStmtCommons, message::Message, statements::Statement, Executor,
      ExecutorBuffer, MessageTy, Postgres, Record, Statements,
    },
    RecordValues, Stmt,
  },
  misc::{PartitionedFilledBuffer, Stream, _read_until},
};
use alloc::vec::Vec;
use core::{borrow::BorrowMut, ops::Range};

impl<E, EB, S> Executor<E, EB, S>
where
  EB: BorrowMut<ExecutorBuffer>,
  S: Stream,
{
  pub(crate) async fn write_send_await_fetch_with_stmt<'rec, STMT, RV>(
    fwsc: &mut FetchWithStmtCommons<'_, S>,
    nb: &'rec mut PartitionedFilledBuffer,
    rv: RV,
    stmt: STMT,
    stmts: &'rec mut Statements,
    vb: &'rec mut Vec<(bool, Range<usize>)>,
  ) -> Result<Record<'rec, E>, E>
  where
    E: From<crate::Error>,
    RV: RecordValues<Postgres<E>>,
    STMT: Stmt,
  {
    let (_, stmt_id_str, stmt) =
      Self::write_send_await_stmt_prot(fwsc, nb, stmt, stmts, vb).await?;
    Self::write_send_await_fetch_with_stmt_wo_prot(fwsc, nb, rv, stmt, &stmt_id_str, vb).await
  }

  pub(crate) async fn write_send_await_fetch_with_stmt_wo_prot<'rec, RV>(
    fwsc: &mut FetchWithStmtCommons<'_, S>,
    nb: &'rec mut PartitionedFilledBuffer,
    rv: RV,
    stmt: Statement<'rec>,
    stmt_id_str: &str,
    vb: &'rec mut Vec<(bool, Range<usize>)>,
  ) -> Result<Record<'rec, E>, E>
  where
    E: From<crate::Error>,
    RV: RecordValues<Postgres<E>>,
  {
    Self::write_send_await_stmt_initial(fwsc, nb, rv, &stmt, &stmt_id_str).await?;
    let mut data_row_msg_range = None;
    loop {
      let msg = Self::fetch_msg_from_stream(fwsc.is_closed, nb, fwsc.stream).await?;
      match msg.ty {
        MessageTy::DataRow(len) => {
          data_row_msg_range = Some((len, nb._current_range()));
        }
        MessageTy::ReadyForQuery => break,
        MessageTy::CommandComplete(_) | MessageTy::EmptyQueryResponse => {}
        _ => return Err(crate::Error::UnexpectedDatabaseMessage { received: msg.tag }.into()),
      }
    }
    if let Some((record_bytes, len)) = data_row_msg_range.and_then(|(len, range)| {
      let record_range = range.start.wrapping_add(7)..range.end;
      Some((nb._buffer().get(record_range)?, len))
    }) {
      Record::parse(record_bytes, 0..record_bytes.len(), stmt, vb, len).map_err(From::from)
    } else {
      Err(crate::Error::NoRecord.into())
    }
  }

  pub(crate) async fn fetch_msg_from_stream<'nb>(
    is_closed: &mut bool,
    nb: &'nb mut PartitionedFilledBuffer,
    stream: &mut S,
  ) -> crate::Result<Message<'nb>> {
    let tag = Self::fetch_representative_msg_from_stream(nb, stream).await?;
    Ok(Message { tag, ty: MessageTy::try_from((is_closed, nb._current()))? })
  }

  async fn fetch_one_header_from_stream(
    nb: &mut PartitionedFilledBuffer,
    read: &mut usize,
    stream: &mut S,
  ) -> crate::Result<(u8, usize)> {
    let buffer = nb._following_trail_mut();
    let [mt_n, b, c, d, e] = _read_until::<5, S>(buffer, read, 0, stream).await?;
    let len: usize = u32::from_be_bytes([b, c, d, e]).try_into()?;
    Ok((mt_n, len.wrapping_add(1)))
  }

  async fn fetch_one_msg_from_stream<'nb>(
    nb: &'nb mut PartitionedFilledBuffer,
    stream: &mut S,
  ) -> crate::Result<u8> {
    let mut read = nb._following_len();
    let (ty, len) = Self::fetch_one_header_from_stream(nb, &mut read, stream).await?;
    Self::fetch_one_payload_from_stream(len, nb, &mut read, stream).await?;
    let current_end_idx = nb._current_end_idx();
    nb._set_indices(current_end_idx, len, read.wrapping_sub(len))?;
    Ok(ty)
  }

  async fn fetch_one_payload_from_stream(
    len: usize,
    nb: &mut PartitionedFilledBuffer,
    read: &mut usize,
    stream: &mut S,
  ) -> crate::Result<()> {
    let mut is_payload_filled = false;
    nb._expand_following(len);
    for _ in 0..len {
      if *read >= len {
        is_payload_filled = true;
        break;
      }
      *read = read.wrapping_add(
        stream.read(nb._following_trail_mut().get_mut(*read..).unwrap_or_default()).await?,
      );
    }
    if !is_payload_filled {
      return Err(crate::Error::UnexpectedBufferState);
    }
    Ok(())
  }

  async fn fetch_representative_msg_from_stream<'nb>(
    nb: &'nb mut PartitionedFilledBuffer,
    stream: &mut S,
  ) -> crate::Result<u8> {
    let mut tag = Self::fetch_one_msg_from_stream(&mut *nb, stream).await?;
    while tag == b'N' {
      tag = Self::fetch_one_msg_from_stream(nb, stream).await?;
    }
    Ok(tag)
  }
}
