//! Different types of redirects.

use wtx::{
  http::{
    server_framework::{get, Redirect, Router, ServerFrameworkBuilder, StateClean},
    ReqResBuffer, StatusCode,
  },
  misc::{simple_seed, Xorshift64},
};

#[tokio::main]
async fn main() -> wtx::Result<()> {
  let router =
    Router::paths(wtx::paths!(("/permanent", get(permanent)), ("/temporary", get(temporary))))?;
  ServerFrameworkBuilder::new(router)
    .without_aux()
    .tokio(
      &wtx_instances::host_from_args(),
      Xorshift64::from(simple_seed()),
      |error| eprintln!("{error}"),
      |_| Ok(()),
    )
    .await
}

async fn permanent() -> Redirect {
  Redirect::permanent("/some/path")
}

async fn temporary(state: StateClean<'_, (), (), ReqResBuffer>) -> wtx::Result<StatusCode> {
  Redirect::temporary_raw(&mut state.req.rrd.headers, "/another/path")
}
