use crate::{
  http::{
    cookie::{CookieGeneric, SameSite},
    session::{SessionKey, SessionManagerInner},
    SessionManager, SessionStore,
  },
  misc::{sleep, Lock, Rng, Vector},
};
use chrono::{DateTime, Utc};
use core::{future::Future, marker::PhantomData, time::Duration};

/// Default and optional parameters for the construction of a [`Session`].
#[derive(Debug)]
pub struct SessionManagerBuilder {
  pub(crate) cookie_def: CookieGeneric<&'static [u8], Vector<u8>>,
  pub(crate) inspection_interval: Duration,
}

impl SessionManagerBuilder {
  #[inline]
  pub(crate) const fn new() -> Self {
    Self {
      cookie_def: CookieGeneric {
        domain: &[],
        expire: None,
        http_only: true,
        max_age: None,
        name: "id".as_bytes(),
        path: "/".as_bytes(),
        same_site: Some(SameSite::Strict),
        secure: true,
        value: Vector::new(),
      },
      inspection_interval: Duration::from_secs(60 * 30),
    }
  }

  /// Creates a new [`Session`] with a random generated key. It is up to the caller to provide
  /// a good RNG.
  ///
  /// The returned [`Future`] is responsible for deleting expired sessions at an interval defined by
  /// [`Self::inspection_interval`] and should be called in a separated task.
  ///
  /// If the backing store already has a system that automatically removes outdated sessions like
  /// SQL triggers, then the [`Future`] can be ignored.
  #[inline]
  pub fn build_generating_key<CS, E, I, RNG, SS>(
    self,
    rng: &mut RNG,
    session_store: &mut SS,
  ) -> (impl Future<Output = Result<(), E>>, SessionManager<I>)
  where
    E: From<crate::Error>,
    I: Lock<Resource = SessionManagerInner<CS, E>>,
    RNG: Rng,
    SS: Clone + SessionStore<CS, E>,
  {
    let mut key = [0; 32];
    rng.fill_slice(&mut key);
    Self::build_with_key(self, key, session_store)
  }

  /// Creates a new [`Session`] with the provided `key`.
  ///
  /// The returned [`Future`] is responsible for deleting expired sessions at an interval defined by
  /// [`Self::inspection_interval`] and should be called in a separated task.
  ///
  /// If the backing store already has a system that automatically removes outdated sessions like
  /// SQL triggers, then the [`Future`] can be ignored.
  #[inline]
  pub fn build_with_key<CS, E, I, SS>(
    self,
    key: SessionKey,
    session_store: &mut SS,
  ) -> (impl Future<Output = Result<(), E>>, SessionManager<I>)
  where
    E: From<crate::Error>,
    I: Lock<Resource = SessionManagerInner<CS, E>>,
    SS: Clone + SessionStore<CS, E>,
  {
    let Self { cookie_def, inspection_interval } = self;
    let mut local_store = session_store.clone();
    (
      async move {
        loop {
          local_store.delete_expired().await?;
          sleep(inspection_interval).await.map_err(Into::into)?;
        }
      },
      SessionManager {
        inner: I::new(SessionManagerInner { cookie_def, phantom: PhantomData, key }),
      },
    )
  }

  /// Defines the host to which the cookie will be sent.
  #[inline]
  pub const fn domain(mut self, elem: &'static [u8]) -> Self {
    self.cookie_def.domain = elem;
    self
  }

  /// Indicates the maximum lifetime of the cookie as an HTTP-date timestamp.
  #[inline]
  pub fn expires(mut self, elem: Option<DateTime<Utc>>) -> Self {
    self.cookie_def.expire = elem;
    self
  }

  /// Forbids JavaScript from accessing the cookie.
  #[inline]
  pub fn http_only(mut self, elem: bool) -> Self {
    self.cookie_def.http_only = elem;
    self
  }

  /// The amount of time the inspection future return by [Self::build] will wait before
  /// deleting expired sessions.
  #[inline]
  pub fn inspection_interval(mut self, elem: Duration) -> Self {
    self.inspection_interval = elem;
    self
  }

  /// Cookie name.
  #[inline]
  pub fn name(mut self, elem: &'static [u8]) -> Self {
    self.cookie_def.name = elem;
    self
  }

  /// Indicates the number of seconds until the cookie expires.
  #[inline]
  pub fn max_age(mut self, elem: Option<Duration>) -> Self {
    self.cookie_def.max_age = elem;
    self
  }

  /// Indicates the path that must exist in the requested URL for the browser to send the Cookie
  /// header.
  #[inline]
  pub fn path(mut self, elem: &'static [u8]) -> Self {
    self.cookie_def.domain = elem;
    self
  }

  /// Controls whether or not a cookie is sent with cross-site requests.
  #[inline]
  pub fn same_site(mut self, elem: Option<SameSite>) -> Self {
    self.cookie_def.same_site = elem;
    self
  }

  /// Indicates that the cookie is sent to the server only when a request is made with the `https`
  /// scheme.
  #[inline]
  pub fn secure(mut self, elem: bool) -> Self {
    self.cookie_def.secure = elem;
    self
  }
}