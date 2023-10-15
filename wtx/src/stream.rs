use crate::AsyncBounds;
use alloc::vec::Vec;
use core::{cmp::Ordering, future::Future};

/// A stream of values produced asynchronously.
pub trait Stream {
  /// Pulls some bytes from this source into the specified buffer, returning how many bytes
  /// were read.
  fn read(&mut self, bytes: &mut [u8]) -> impl AsyncBounds + Future<Output = crate::Result<usize>>;

  /// Attempts to write all elements of `bytes`.
  fn write_all(&mut self, bytes: &[u8]) -> impl AsyncBounds + Future<Output = crate::Result<()>>;
}

impl Stream for () {
  #[inline]
  fn read(&mut self, _: &mut [u8]) -> impl AsyncBounds + Future<Output = crate::Result<usize>> {
    async { Ok(0) }
  }

  #[inline]
  fn write_all(&mut self, _: &[u8]) -> impl AsyncBounds + Future<Output = crate::Result<()>> {
    async { Ok(()) }
  }
}

impl<T> Stream for &mut T
where
  T: Stream,
{
  #[inline]
  fn read(&mut self, bytes: &mut [u8]) -> impl AsyncBounds + Future<Output = crate::Result<usize>> {
    (*self).read(bytes)
  }

  #[inline]
  fn write_all(&mut self, bytes: &[u8]) -> impl AsyncBounds + Future<Output = crate::Result<()>> {
    (*self).write_all(bytes)
  }
}

/// Stores written data to transfer when read.
#[derive(Debug, Default)]
pub struct BytesStream {
  buffer: Vec<u8>,
  idx: usize,
}

impl BytesStream {
  /// Empties the internal buffer.
  #[inline]
  pub fn clear(&mut self) {
    self.buffer.clear();
    self.idx = 0;
  }
}

impl Stream for BytesStream {
  #[inline]
  fn read(&mut self, bytes: &mut [u8]) -> impl AsyncBounds + Future<Output = crate::Result<usize>> {
    async {
      let working_buffer = self.buffer.get(self.idx..).unwrap_or_default();
      let working_buffer_len = working_buffer.len();
      Ok(match working_buffer_len.cmp(&bytes.len()) {
        Ordering::Less => {
          bytes.get_mut(..working_buffer_len).unwrap_or_default().copy_from_slice(working_buffer);
          self.clear();
          working_buffer_len
        }
        Ordering::Equal => {
          bytes.copy_from_slice(working_buffer);
          self.clear();
          working_buffer_len
        }
        Ordering::Greater => {
          bytes.copy_from_slice(working_buffer.get(..bytes.len()).unwrap_or_default());
          self.idx = self.idx.wrapping_add(bytes.len());
          bytes.len()
        }
      })
    }
  }

  #[inline]
  fn write_all(&mut self, bytes: &[u8]) -> impl AsyncBounds + Future<Output = crate::Result<()>> {
    async {
      self.buffer.extend_from_slice(bytes);
      Ok(())
    }
  }
}

#[cfg(feature = "async-std")]
mod async_std {
  use crate::{AsyncBounds, Stream};
  use async_std::{
    io::{ReadExt, WriteExt},
    net::TcpStream,
  };
  use core::future::Future;

  impl Stream for TcpStream {
    #[inline]
    fn read(
      &mut self,
      bytes: &mut [u8],
    ) -> impl AsyncBounds + Future<Output = crate::Result<usize>> {
      async { Ok(<Self as ReadExt>::read(self, bytes).await?) }
    }

    #[inline]
    fn write_all(&mut self, bytes: &[u8]) -> impl AsyncBounds + Future<Output = crate::Result<()>> {
      async {
        <Self as WriteExt>::write_all(self, bytes).await?;
        Ok(())
      }
    }
  }
}

#[cfg(all(feature = "glommio", not(feature = "async-send")))]
mod glommio {
  use crate::{AsyncBounds, Stream};
  use core::future::Future;
  use futures_lite::io::{AsyncReadExt, AsyncWriteExt};
  use glommio::net::TcpStream;

  impl Stream for TcpStream {
    #[inline]
    fn read(
      &mut self,
      bytes: &mut [u8],
    ) -> impl AsyncBounds + Future<Output = crate::Result<usize>> {
      async { Ok(<Self as AsyncReadExt>::read(self, bytes).await?) }
    }

    #[inline]
    fn write_all(&mut self, bytes: &[u8]) -> impl AsyncBounds + Future<Output = crate::Result<()>> {
      async {
        <Self as AsyncWriteExt>::write_all(self, bytes).await?;
        Ok(())
      }
    }
  }
}

#[cfg(feature = "smol")]
mod smol {
  use crate::{AsyncBounds, Stream};
  use core::future::Future;
  use smol::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
  };

  impl Stream for TcpStream {
    #[inline]
    fn read(
      &mut self,
      bytes: &mut [u8],
    ) -> impl AsyncBounds + Future<Output = crate::Result<usize>> {
      async { Ok(<Self as AsyncReadExt>::read(self, bytes).await?) }
    }

    #[inline]
    fn write_all(&mut self, bytes: &[u8]) -> impl AsyncBounds + Future<Output = crate::Result<()>> {
      async {
        <Self as AsyncWriteExt>::write_all(self, bytes).await?;
        Ok(())
      }
    }
  }
}

#[cfg(feature = "std")]
mod std {
  use crate::{AsyncBounds, Stream};
  use core::future::Future;
  use std::{
    io::{Read, Write},
    net::TcpStream,
  };

  impl Stream for TcpStream {
    #[inline]
    fn read(
      &mut self,
      bytes: &mut [u8],
    ) -> impl AsyncBounds + Future<Output = crate::Result<usize>> {
      async { Ok(<Self as Read>::read(self, bytes)?) }
    }

    #[inline]
    fn write_all(&mut self, bytes: &[u8]) -> impl AsyncBounds + Future<Output = crate::Result<()>> {
      async {
        <Self as Write>::write_all(self, bytes)?;
        Ok(())
      }
    }
  }
}

#[cfg(feature = "tokio")]
mod tokio {
  use crate::{AsyncBounds, Stream};
  use core::future::Future;
  use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
  };

  impl Stream for TcpStream {
    #[inline]
    fn read(
      &mut self,
      bytes: &mut [u8],
    ) -> impl AsyncBounds + Future<Output = crate::Result<usize>> {
      async { Ok(<Self as AsyncReadExt>::read(self, bytes).await?) }
    }

    #[inline]
    fn write_all(&mut self, bytes: &[u8]) -> impl AsyncBounds + Future<Output = crate::Result<()>> {
      async {
        <Self as AsyncWriteExt>::write_all(self, bytes).await?;
        Ok(())
      }
    }
  }
}

#[cfg(feature = "tokio-rustls")]
mod tokio_rustls {
  use crate::{AsyncBounds, Stream};
  use core::future::Future;
  use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

  impl<T> Stream for tokio_rustls::client::TlsStream<T>
  where
    T: AsyncBounds + AsyncRead + AsyncWrite + Unpin,
  {
    #[inline]
    fn read(
      &mut self,
      bytes: &mut [u8],
    ) -> impl AsyncBounds + Future<Output = crate::Result<usize>> {
      async { Ok(<Self as AsyncReadExt>::read(self, bytes).await?) }
    }

    #[inline]
    fn write_all(&mut self, bytes: &[u8]) -> impl AsyncBounds + Future<Output = crate::Result<()>> {
      async {
        <Self as AsyncWriteExt>::write_all(self, bytes).await?;
        Ok(())
      }
    }
  }

  impl<T> Stream for tokio_rustls::server::TlsStream<T>
  where
    T: AsyncBounds + AsyncRead + AsyncWrite + Unpin,
  {
    #[inline]
    fn read(
      &mut self,
      bytes: &mut [u8],
    ) -> impl AsyncBounds + Future<Output = crate::Result<usize>> {
      async { Ok(<Self as AsyncReadExt>::read(self, bytes).await?) }
    }

    #[inline]
    fn write_all(&mut self, bytes: &[u8]) -> impl AsyncBounds + Future<Output = crate::Result<()>> {
      async {
        <Self as AsyncWriteExt>::write_all(self, bytes).await?;
        Ok(())
      }
    }
  }
}
