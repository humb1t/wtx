use crate::misc::_atoi;
use core::any::type_name;

#[derive(Debug)]
pub(crate) enum Authentication<'bytes> {
  Ok,
  Md5Password([u8; 4]),
  Sasl(&'bytes [u8]),
  SaslContinue { iterations: u32, payload: &'bytes [u8], nonce: &'bytes [u8], salt: &'bytes [u8] },
  SaslFinal(&'bytes [u8]),
}

impl<'bytes> TryFrom<&'bytes [u8]> for Authentication<'bytes> {
  type Error = crate::Error;
  fn try_from(bytes: &'bytes [u8]) -> Result<Self, Self::Error> {
    let (n, rest) = if let [a, b, c, d, rest @ ..] = bytes {
      (u32::from_be_bytes([*a, *b, *c, *d]), rest)
    } else {
      return Err(crate::Error::UnexpectedValueFromBytes { expected: type_name::<Self>() });
    };
    Ok(match n {
      0 => Self::Ok,
      5 => Self::Md5Password(rest.try_into()?),
      10 => Self::Sasl(rest),
      11 => {
        let mut iter = rest.split(|b| *b == b',');
        let mut iterations = None;
        let mut nonce = None;
        let mut salt = None;
        while let Some([key, _, local_rest @ ..]) = iter.next() {
          match key {
            b'i' => {
              iterations = Some(_atoi(local_rest)?);
            }
            b'r' => {
              nonce = Some(local_rest);
            }
            b's' => {
              salt = Some(local_rest);
            }
            _ => {}
          }
        }
        Self::SaslContinue {
          iterations: iterations.ok_or(crate::Error::NoInnerValue("iterations"))?,
          nonce: nonce.ok_or(crate::Error::NoInnerValue("nonce"))?,
          payload: rest,
          salt: salt.ok_or(crate::Error::NoInnerValue("salt"))?,
        }
      }
      12 => {
        let mut iter = rest.split(|b| *b == b',');
        let mut verifier = None;
        while let Some([b'v', _, local_rest @ ..]) = iter.next() {
          verifier = Some(local_rest);
        }
        Self::SaslFinal(verifier.ok_or(crate::Error::NoInnerValue("verifier"))?)
      }
      _ => return Err(crate::Error::UnexpectedValueFromBytes { expected: type_name::<Self>() }),
    })
  }
}
