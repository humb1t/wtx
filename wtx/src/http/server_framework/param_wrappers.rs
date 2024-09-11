mod path_owned;
mod path_str;
#[cfg(feature = "serde_json")]
mod serde_json;

use crate::{
  http::HttpError,
  misc::{bytes_split1, UriRef},
};
pub use path_owned::PathOwned;
pub use path_str::PathStr;
#[cfg(feature = "serde_json")]
pub use serde_json::SerdeJson;

#[inline]
fn manage_path<'uri>(
  path_defs: (u8, &[(&'static str, u8)]),
  uri: &'uri UriRef<'_>,
) -> crate::Result<&'uri str> {
  let fun = || {
    let path = uri.path();
    let mut prev_idx: usize = 0;
    let mut iter = path_defs.1.iter().map(|el| el.0.as_bytes());
    while let Some([b'/', sub_path_def @ ..]) = iter.next() {
      prev_idx = prev_idx.wrapping_add(1);
      let has_placeholder = bytes_split1(sub_path_def, b'/').any(|elem| {
        if let [b'{', ..] = elem {
          prev_idx = prev_idx.wrapping_add(1);
          true
        } else {
          prev_idx = prev_idx.wrapping_add(elem.len());
          false
        }
      });
      if !has_placeholder {
        continue;
      };
      return path.get(prev_idx..);
    }
    None
  };
  fun().ok_or_else(|| crate::Error::from(HttpError::UriMismatch))
}
