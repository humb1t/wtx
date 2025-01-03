/// Generic parameter used in different buffer operations
#[derive(Clone, Copy, Debug)]
pub enum BufferMode {
  /// Buffer should be modified with more elements
  Additional(usize),
  /// Buffer should be modified using the exact specified length.
  Len(usize),
}

impl BufferMode {
  #[inline]
  pub(crate) fn params(self, len: usize) -> Option<(usize, usize)> {
    Some(match self {
      BufferMode::Additional(elem) => (elem, len.checked_add(elem)?),
      BufferMode::Len(elem) => (elem.checked_sub(len)?, elem),
    })
  }
}
