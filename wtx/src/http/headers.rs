use crate::misc::{BytesFmt, Lease, LeaseMut, Vector};
use core::{fmt::Arguments, ptr};

/// Determines how trailers are placed in the headers
#[derive(Clone, Copy, Debug)]
pub enum Trailers {
  /// Does not have trailers
  None,
  /// Trailers are arbitrary placed inside the headers
  Mixed,
  /// All trailers are positioned at the end of the headers
  Tail(usize),
}

impl Trailers {
  /// If there is at least one trailer header, of any type.
  #[inline]
  pub fn has_any(self) -> bool {
    matches!(self, Trailers::Mixed | Trailers::Tail(_))
  }
}

/// List of pairs sent and received on every request/response.
#[derive(Debug)]
pub struct Headers {
  bytes: Vector<u8>,
  headers_parts: Vector<HeaderParts>,
  trailers: Trailers,
}

impl Headers {
  /// Empty instance
  #[inline]
  pub const fn new() -> Self {
    Self { bytes: Vector::new(), headers_parts: Vector::new(), trailers: Trailers::None }
  }

  /// Pre-allocates bytes according to the number of passed elements.
  ///
  /// Bytes are capped according to the specified `max_bytes`.
  #[inline]
  pub fn with_capacity(bytes: usize, headers: usize) -> crate::Result<Self> {
    Ok(Self {
      bytes: Vector::with_capacity(bytes)?,
      headers_parts: Vector::with_capacity(headers)?,
      trailers: Trailers::None,
    })
  }

  /// The amount of bytes used by all of the headers
  #[inline]
  pub fn bytes_len(&self) -> usize {
    self.bytes.len()
  }

  /// Clears the internal buffer "erasing" all previously inserted elements.
  ///
  /// ```rust
  /// use wtx::http::{Header, Headers};
  /// let mut headers = Headers::new();
  /// headers.push_from_iter(Header::from_name_and_value(b"name", ["value".as_bytes()])).unwrap();
  /// assert_eq!(headers.bytes_len(), 9);
  /// assert_eq!(headers.headers_len(), 1);
  /// headers.clear();
  /// assert_eq!(headers.bytes_len(), 0);
  /// assert_eq!(headers.headers_len(), 0);
  /// ```
  #[inline]
  pub fn clear(&mut self) {
    let Self { bytes, headers_parts, trailers } = self;
    bytes.clear();
    headers_parts.clear();
    *trailers = Trailers::None;
  }

  /// Returns the header that is referenced by `idx`, if any.
  #[inline]
  pub fn get_by_idx(&self, idx: usize) -> Option<Header<'_, &[u8]>> {
    self.headers_parts.get(idx).copied().map(|header_parts| Self::map(&self.bytes, header_parts))
  }

  /// Returns the header that is referenced by `name`, if any.
  #[inline]
  pub fn get_by_name(&self, name: &[u8]) -> Option<Header<'_, &[u8]>> {
    self.iter().find(|el| el.name == name)
  }

  /// Returns all first optional headers that are referenced by `names`.
  ///
  /// ```rust
  /// use wtx::http::{Header, Headers};
  /// let mut headers = Headers::new();
  /// headers.push_from_iter(Header::from_name_and_value(b"name0", [])).unwrap();
  /// let array = headers.get_many_by_name(["name0".as_bytes(), "name1".as_bytes()]);
  /// assert!(array[0].is_some());
  /// assert!(array[1].is_none());
  /// ```
  #[inline]
  pub fn get_many_by_name<const N: usize>(
    &self,
    names: [&[u8]; N],
  ) -> [Option<Header<'_, &[u8]>>; N] {
    let mut rslt = [None; N];
    for (header, value) in self.iter().zip(&mut rslt) {
      if names.iter().any(|name| *name == header.name) {
        *value = Some(header);
      }
    }
    rslt
  }

  /// The number of headers
  #[inline]
  pub fn headers_len(&self) -> usize {
    self.headers_parts.len()
  }

  /// Retrieves all stored pairs.
  #[inline]
  pub fn iter(&self) -> impl Iterator<Item = Header<'_, &[u8]>> {
    self.headers_parts.iter().copied().map(|header_parts| Self::map(&self.bytes, header_parts))
  }

  /// Removes the last element.
  ///
  /// ```rust
  /// use wtx::http::{Header, Headers};
  /// let mut headers = Headers::new();
  /// headers.push_from_iter(Header::from_name_and_value(b"name", ["value".as_bytes()])).unwrap();
  /// assert_eq!(headers.bytes_len(), 9);
  /// assert_eq!(headers.headers_len(), 1);
  /// let _ = headers.pop();
  /// assert_eq!(headers.bytes_len(), 0);
  /// assert_eq!(headers.headers_len(), 0);
  /// ```
  #[inline]
  pub fn pop(&mut self) -> bool {
    let Some(header_parts) = self.headers_parts.pop() else {
      return false;
    };
    let new_bytes_len = self.bytes.len().wrapping_sub(header_parts.header_len);
    // SAFETY: `headers` is expected to contain valid data
    unsafe {
      self.bytes.set_len(new_bytes_len);
    }
    true
  }

  /// Pushes a new header with its value composed by [`Arguments`].
  ///
  /// ```
  /// use wtx::http::{Header, Headers};
  /// let mut headers = Headers::new();
  /// headers.push_from_fmt(Header::from_name_and_value(b"name", format_args!("{}", 1))).unwrap();
  /// assert_eq!(headers.get_by_idx(0).unwrap(), Header::from_name_and_value(b"name", "1".as_bytes()));
  /// ```
  #[inline(always)]
  pub fn push_from_fmt(&mut self, header: Header<'_, Arguments<'_>>) -> crate::Result<()> {
    let header_begin = self.bytes.len();
    #[cfg(feature = "std")]
    {
      use std::io::Write;
      self.bytes.write_fmt(format_args!("{}{}", BytesFmt(header.name), header.value))?;
    }
    #[cfg(not(feature = "std"))]
    {
      use core::fmt::Write;
      self.bytes.write_fmt(format_args!("{}{}", BytesFmt(header.name), header.value))?;
    }
    let prev_len = self.headers_parts.len();
    self.headers_parts.push(HeaderParts {
      header_begin,
      header_end: self.bytes.len(),
      header_len: self.bytes.len().wrapping_sub(header_begin),
      header_name_end: header_begin.wrapping_add(header.name.len()),
      is_sensitive: header.is_sensitive,
      is_trailer: header.is_trailer,
    })?;
    Self::manage_trailers(header.is_trailer, prev_len, &mut self.trailers);
    Ok(())
  }

  /// Pushes a new header with its value composed by several slices.
  ///
  /// ```
  /// use wtx::http::{Header, Headers};
  /// let mut headers = Headers::new();
  /// headers.push_from_iter(Header::from_name_and_value(b"name", ["value0".as_bytes(), "_value1".as_bytes()])).unwrap();
  /// assert_eq!(headers.get_by_idx(0).unwrap(), Header::from_name_and_value(b"name", "value0_value1".as_bytes()));
  /// ```
  #[inline(always)]
  pub fn push_from_iter<'bytes, V>(&mut self, header: Header<'bytes, V>) -> crate::Result<()>
  where
    V: IntoIterator<Item = &'bytes [u8]>,
    V::IntoIter: Clone,
  {
    // FIXME(MIRI): a function, even it it is #[inline], breaks the tracking of the pointer
    macro_rules! copy {
      ($bytes:expr, $header_end:expr, $value:expr) => {{
        // SAFETY: `header_end is within bounds`
        let dst = unsafe { $bytes.as_mut_ptr().add(*$header_end) };
        // SAFETY: `reserve` allocated memory
        unsafe {
          ptr::copy_nonoverlapping($value.as_ptr(), dst, $value.len());
        }
        *$header_end = $header_end.wrapping_add($value.len());
      }};
    }

    let iter = header.value.into_iter();
    let header_len = Self::header_len(header.name, iter.clone());
    self.reserve(header_len, 1)?;
    let header_begin = self.bytes.len();
    let mut header_end = header_begin;
    copy!(&mut self.bytes, &mut header_end, header.name);
    let header_name_end = header_end;
    for value in iter {
      copy!(&mut self.bytes, &mut header_end, value);
    }
    // SAFETY: `header_end is within bounds`
    unsafe {
      self.bytes.set_len(header_end);
    }
    let prev_len = self.headers_parts.len();
    self.headers_parts.push(HeaderParts {
      header_begin,
      header_end,
      header_len,
      header_name_end,
      is_sensitive: header.is_sensitive,
      is_trailer: header.is_trailer,
    })?;
    Self::manage_trailers(header.is_trailer, prev_len, &mut self.trailers);
    Ok(())
  }

  /// Similarly to [`Self::push_from_iter`], pushes several headers.
  #[inline]
  pub fn push_from_iter_many<'bytes, const N: usize, V>(
    &mut self,
    headers: [Header<'bytes, V>; N],
  ) -> crate::Result<()>
  where
    V: Clone + Iterator<Item = &'bytes [u8]>,
  {
    let mut header_len: usize = 0;
    for header in &headers {
      header_len = header_len.wrapping_add(Self::header_len(header.name, header.value.clone()));
    }
    self.reserve(header_len, N)?;
    for header in headers {
      self.push_from_iter(header)?;
    }
    Ok(())
  }

  /// Reserves capacity for at least `bytes` more bytes to be inserted. The same thing is applied
  /// to the number of headers.
  ///
  /// Bytes are capped according to the specified `max_bytes`.
  #[inline(always)]
  pub fn reserve(&mut self, bytes: usize, headers: usize) -> crate::Result<()> {
    self.bytes.reserve(bytes)?;
    self.headers_parts.reserve(headers)?;
    Ok(())
  }

  /// If this instance has one or more trailer headers.
  #[inline]
  pub fn trailers(&self) -> Trailers {
    self.trailers
  }

  #[inline]
  fn header_len<'bytes>(header_name: &[u8], iter: impl Iterator<Item = &'bytes [u8]>) -> usize {
    let mut header_len = header_name.len();
    for elem in iter {
      header_len = header_len.wrapping_add(elem.len());
    }
    header_len
  }

  #[inline]
  fn manage_trailers(is_trailer: bool, prev_len: usize, trailers: &mut Trailers) {
    *trailers = if is_trailer {
      match trailers {
        Trailers::Mixed => Trailers::Mixed,
        Trailers::None => Trailers::Tail(prev_len),
        Trailers::Tail(idx) => Trailers::Tail(*idx),
      }
    } else {
      match trailers {
        Trailers::Mixed | Trailers::Tail(_) => Trailers::Mixed,
        Trailers::None => Trailers::None,
      }
    };
  }

  #[inline]
  fn map(bytes: &[u8], header_parts: HeaderParts) -> Header<'_, &[u8]> {
    let HeaderParts {
      header_begin,
      header_end,
      header_name_end,
      header_len: _,
      is_sensitive,
      is_trailer,
    } = header_parts;
    Header {
      is_sensitive,
      is_trailer,
      name: bytes.get(header_begin..header_name_end).unwrap_or_default(),
      value: bytes.get(header_name_end..header_end).unwrap_or_default(),
    }
  }
}

impl Lease<Headers> for Headers {
  #[inline]
  fn lease(&self) -> &Headers {
    self
  }
}

impl LeaseMut<Headers> for Headers {
  #[inline]
  fn lease_mut(&mut self) -> &mut Headers {
    self
  }
}

impl Default for Headers {
  #[inline]
  fn default() -> Self {
    Self::new()
  }
}

/// A field of an HTTP request or response.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Header<'any, V> {
  /// If the name/value should NOT be cached.
  ///
  /// The applicability of this parameter depends on the HTTP version.
  pub is_sensitive: bool,
  /// Trailers are added at the end of a message.
  ///
  /// The applicability and semantics depends on the HTTP version.
  pub is_trailer: bool,
  /// Header name
  pub name: &'any [u8],
  /// Header value
  pub value: V,
}

impl<'any, V> Header<'any, V> {
  /// Sets `is_sensitive` and `is_trailer` to `false`.
  #[inline]
  pub fn from_name_and_value(name: &'any [u8], value: V) -> Self {
    Self { is_sensitive: false, is_trailer: false, name, value }
  }
}

#[derive(Clone, Copy, Debug)]
struct HeaderParts {
  header_begin: usize,
  header_end: usize,
  header_len: usize,
  header_name_end: usize,
  is_sensitive: bool,
  is_trailer: bool,
}
