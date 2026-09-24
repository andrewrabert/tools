use anyhow::Result;
use reqwest::blocking::ClientBuilder;

use crate::{APP_NAME, APP_VERSION};

/// Returns a blocking client builder with the shared user agent and TLS roots.
pub fn client_builder() -> Result<ClientBuilder> {
    let builder = ClientBuilder::new().user_agent(format!("{APP_NAME}/{APP_VERSION}"));
    // rustls-platform-verifier needs a JVM on Android, which a CLI does not have.
    #[cfg(target_os = "android")]
    let builder = builder.tls_certs_only(
        webpki_root_certs::TLS_SERVER_ROOT_CERTS
            .iter()
            .map(|der| reqwest::Certificate::from_der(der))
            .collect::<reqwest::Result<Vec<_>>>()?,
    );
    Ok(builder)
}
