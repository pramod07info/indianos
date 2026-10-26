//! R038/38: lawful macOS compatibility route guidance, not macOS emulation.
#[derive(Debug,PartialEq,Eq)]pub enum Route{PortableRebuild,Web,AuthorizedRemote}
#[derive(Debug,PartialEq,Eq)]pub enum Error{Unavailable,Unauthorized}
pub struct App{pub source_portable:bool,pub web_app:bool,
 pub remote_license:bool,pub authorized_host:bool,pub proprietary_frameworks:bool}
pub fn choose(app:App)->Result<Route,Error>{
 if app.source_portable&&!app.proprietary_frameworks{return Ok(Route::PortableRebuild);}
 if app.web_app{return Ok(Route::Web);}
 if app.remote_license&&app.authorized_host{return Ok(Route::AuthorizedRemote);}
 if app.proprietary_frameworks{return Err(Error::Unauthorized);}
 Err(Error::Unavailable)
}
#[cfg(test)]mod tests{use super::*;
 #[test]fn no_unlicensed_mac_runtime(){
  assert_eq!(choose(App{source_portable:false,web_app:false,remote_license:false,
   authorized_host:false,proprietary_frameworks:true}),Err(Error::Unauthorized));
 }
}