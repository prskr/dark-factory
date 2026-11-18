//! TLS infrastructure shared by the control plane and the gateway: a mutual-TLS acceptor that
//! reloads rotated certificates, and the identity of a sandbox pod from its client certificate.
//!
//! TLS is terminated here, not by tonic, so the certificate, key and client CA can be reloaded
//! when cert-manager rotates the mounted secret, without restarting the pod or dropping streams.

mod identity;
mod reload;
#[cfg(feature = "test-util")]
pub mod testing;

pub use identity::{IdentityError, pod_uri_san};
pub use reload::{MtlsIncoming, TlsError, TlsPaths, TlsReloader, mtls_incoming};
