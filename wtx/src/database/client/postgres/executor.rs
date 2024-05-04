mod authentication;
mod commons;
mod fetch;
mod prepare;
mod simple_query;

use crate::{
  database::{
    client::postgres::{
      encrypted_conn,
      executor::commons::FetchWithStmtCommons,
      executor_buffer::{ExecutorBuffer, ExecutorBufferPartsMut},
      initial_conn_msg, Config, MessageTy, Postgres, Record, Records, TransactionManager,
    },
    Database, RecordValues, StmtCmd, TransactionManager as _,
  },
  misc::{AsyncBounds, FilledBufferWriter, Lease, LeaseMut, Stream, TlsStream},
  rng::Rng,
};
use core::{future::Future, marker::PhantomData};

/// Executor
#[derive(Debug)]
pub struct Executor<E, EB, S> {
  pub(crate) eb: EB,
  pub(crate) is_closed: bool,
  pub(crate) phantom: PhantomData<E>,
  pub(crate) stream: S,
}

impl<E, EB, S> Executor<E, EB, S>
where
  EB: LeaseMut<ExecutorBuffer>,
  S: Stream,
{
  /// Connects with an unencrypted stream.
  #[inline]
  pub async fn connect<RNG>(
    config: &Config<'_>,
    mut eb: EB,
    rng: &mut RNG,
    stream: S,
  ) -> crate::Result<Self>
  where
    RNG: Rng,
  {
    eb.lease_mut().clear();
    Self::do_connect(config, eb, rng, stream, None).await
  }

  /// Initially connects with an unencrypted stream that should be later upgraded to an encrypted
  /// stream.
  #[inline]
  pub async fn connect_encrypted<F, IS, RNG>(
    config: &Config<'_>,
    mut eb: EB,
    mut initial_stream: IS,
    rng: &mut RNG,
    cb: impl FnOnce(IS) -> F,
  ) -> crate::Result<Self>
  where
    F: Future<Output = crate::Result<S>>,
    IS: Stream,
    RNG: Rng,
    S: TlsStream,
  {
    eb.lease_mut().clear();
    let mut fbw = FilledBufferWriter::from(&mut eb.lease_mut().nb);
    encrypted_conn(&mut fbw)?;
    initial_stream.write_all(fbw._curr_bytes()).await?;
    let mut buf = [0];
    let _ = initial_stream.read(&mut buf).await?;
    if buf[0] != b'S' {
      return Err(crate::Error::ServerDoesNotSupportEncryption);
    }
    let stream = cb(initial_stream).await?;
    let tls_server_end_point = stream.tls_server_end_point()?;
    Self::do_connect(config, eb, rng, stream, tls_server_end_point.as_ref().map(Lease::lease)).await
  }

  /// Mutable buffer reference
  #[inline]
  pub fn eb_mut(&mut self) -> &mut ExecutorBuffer {
    self.eb.lease_mut()
  }

  #[inline]
  async fn do_connect<RNG>(
    config: &Config<'_>,
    eb: EB,
    rng: &mut RNG,
    stream: S,
    tls_server_end_point: Option<&[u8]>,
  ) -> crate::Result<Self>
  where
    RNG: Rng,
  {
    let mut this = Self { eb, is_closed: false, phantom: PhantomData, stream };
    this.send_initial_conn_msg(config).await?;
    this.manage_authentication(config, rng, tls_server_end_point).await?;
    this.read_after_authentication_data().await?;
    Ok(this)
  }

  async fn send_initial_conn_msg(&mut self, config: &Config<'_>) -> crate::Result<()> {
    let mut fbw = FilledBufferWriter::from(&mut self.eb.lease_mut().nb);
    initial_conn_msg(config, &mut fbw)?;
    self.stream.write_all(fbw._curr_bytes()).await?;
    Ok(())
  }
}

impl<E, EB, S> crate::database::Executor for Executor<E, EB, S>
where
  E: AsyncBounds + From<crate::Error>,
  EB: AsyncBounds + LeaseMut<ExecutorBuffer>,
  S: AsyncBounds + Stream,
{
  type Database = Postgres<E>;
  type TransactionManager<'tm> = TransactionManager<'tm, E, EB, S>
  where
    Self: 'tm;

  #[inline]
  async fn execute(&mut self, cmd: &str, cb: impl FnMut(u64)) -> crate::Result<()> {
    self.simple_query_execute(cmd, cb).await
  }

  #[inline]
  async fn execute_with_stmt<SC, RV>(
    &mut self,
    sc: SC,
    rv: RV,
  ) -> Result<u64, <Self::Database as Database>::Error>
  where
    RV: RecordValues<Self::Database>,
    SC: StmtCmd,
  {
    let ExecutorBufferPartsMut { nb, rb, stmts, vb, .. } = self.eb.lease_mut().parts_mut();
    ExecutorBuffer::clear_cmd_buffers(nb, rb, vb);
    let mut rows = 0;
    let mut fwsc = FetchWithStmtCommons {
      is_closed: &mut self.is_closed,
      rb,
      stream: &mut self.stream,
      tys: &[],
    };
    let (_, stmt_id_str, stmt) =
      Self::write_send_await_stmt_prot(&mut fwsc, nb, sc, stmts, vb).await?;
    Self::write_send_await_stmt_initial(&mut fwsc, nb, rv, &stmt, &stmt_id_str).await?;
    loop {
      let msg = Self::fetch_msg_from_stream(fwsc.is_closed, nb, fwsc.stream).await?;
      match msg.ty {
        MessageTy::CommandComplete(local_rows) => {
          rows = local_rows;
        }
        MessageTy::ReadyForQuery => break,
        MessageTy::DataRow(_) | MessageTy::EmptyQueryResponse => {}
        _ => return Err(crate::Error::UnexpectedDatabaseMessage { received: msg.tag }.into()),
      }
    }
    Ok(rows)
  }

  #[inline]
  async fn fetch_with_stmt<SC, RV>(
    &mut self,
    sc: SC,
    rv: RV,
  ) -> Result<<Self::Database as Database>::Record<'_>, E>
  where
    RV: RecordValues<Self::Database>,
    SC: StmtCmd,
  {
    let ExecutorBufferPartsMut { nb, rb, stmts, vb, .. } = self.eb.lease_mut().parts_mut();
    Self::write_send_await_fetch_with_stmt(
      &mut FetchWithStmtCommons {
        is_closed: &mut self.is_closed,
        rb,
        stream: &mut self.stream,
        tys: &[],
      },
      nb,
      rv,
      sc,
      stmts,
      vb,
    )
    .await
  }

  #[inline]
  async fn fetch_many_with_stmt<SC, RV>(
    &mut self,
    sc: SC,
    rv: RV,
    mut cb: impl AsyncBounds + FnMut(&<Self::Database as Database>::Record<'_>) -> Result<(), E>,
  ) -> Result<<Self::Database as Database>::Records<'_>, E>
  where
    RV: AsyncBounds + RecordValues<Self::Database>,
    SC: AsyncBounds + StmtCmd,
  {
    let ExecutorBufferPartsMut { nb, rb, stmts, vb, .. } = self.eb.lease_mut().parts_mut();
    ExecutorBuffer::clear_cmd_buffers(nb, rb, vb);
    let mut fwsc = FetchWithStmtCommons {
      is_closed: &mut self.is_closed,
      rb,
      stream: &mut self.stream,
      tys: &[],
    };
    let (_, stmt_id_str, stmt) =
      Self::write_send_await_stmt_prot(&mut fwsc, nb, sc, stmts, vb).await?;
    Self::write_send_await_stmt_initial(&mut fwsc, nb, rv, &stmt, &stmt_id_str).await?;
    let begin = nb._current_end_idx();
    let begin_data = nb._current_end_idx().wrapping_add(7);
    loop {
      let msg = Self::fetch_msg_from_stream(fwsc.is_closed, nb, fwsc.stream).await?;
      match msg.ty {
        MessageTy::DataRow(len) => {
          let bytes = nb._buffer().get(begin_data..nb._current_end_idx()).unwrap_or_default();
          let range_begin = nb._antecedent_end_idx().wrapping_sub(begin);
          let range_end = nb._current_end_idx().wrapping_sub(begin_data);
          cb(&Record::parse(bytes, range_begin..range_end, stmt.clone(), vb, len)?)?;
          fwsc.rb.push(vb.len());
        }
        MessageTy::ReadyForQuery => {
          break;
        }
        MessageTy::CommandComplete(_) | MessageTy::EmptyQueryResponse => {}
        _ => {
          return Err(crate::Error::UnexpectedDatabaseMessage { received: msg.tag }.into());
        }
      }
    }
    Ok(Records {
      bytes: nb
        ._buffer()
        .get(begin_data.wrapping_add(4)..nb._current_end_idx())
        .unwrap_or_default(),
      phantom: PhantomData,
      records_values_offsets: rb,
      stmt,
      values_bytes_offsets: vb,
    })
  }

  #[inline]
  fn is_closed(&self) -> bool {
    self.is_closed
  }

  #[inline]
  async fn prepare(&mut self, cmd: &str) -> Result<u64, E> {
    let ExecutorBufferPartsMut { nb, rb, stmts, vb, .. } = self.eb.lease_mut().parts_mut();
    ExecutorBuffer::clear_cmd_buffers(nb, rb, vb);
    Ok(
      Self::write_send_await_stmt_prot(
        &mut FetchWithStmtCommons {
          is_closed: &mut self.is_closed,
          rb,
          stream: &mut self.stream,
          tys: &[],
        },
        nb,
        cmd,
        stmts,
        vb,
      )
      .await?
      .0,
    )
  }

  #[inline]
  async fn transaction(&mut self) -> crate::Result<Self::TransactionManager<'_>> {
    let ExecutorBufferPartsMut { nb, rb, vb, .. } = self.eb.lease_mut().parts_mut();
    ExecutorBuffer::clear_cmd_buffers(nb, rb, vb);
    let mut tm = TransactionManager::new(self);
    tm.begin().await?;
    Ok(tm)
  }
}
