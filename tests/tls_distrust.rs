//! Run with: cargo test --locked --test tls_distrust -- --ignored --nocapture
use std::{error::Error, time::Duration};

const DISTRUSTED_ENDPOINTS: [&str; 2] = [
    "https://dv-g3b-emudhra.we-have-a-problem.com/",
    "https://dv-g3b-aaa-emudhra.we-have-a-problem.com/",
];

#[tokio::test]
#[ignore = "requires direct internet access to the public certificate test endpoints"]
async fn rejects_compromised_emudhra_certificate_chains() {
    // Use the application's reqwest/Rustls dependencies. Direct connections and
    // disabled redirects ensure the named endpoint's certificate is checked.
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(20))
        .build()
        .expect("HTTP client must build");

    client
        .get("https://www.rust-lang.org/")
        .send()
        .await
        .expect("positive control must establish a trusted HTTPS connection");
    println!("Trusted HTTPS positive control accepted");

    for endpoint in DISTRUSTED_ENDPOINTS {
        let error = client
            .get(endpoint)
            .send()
            .await
            .expect_err("compromised certificates must fail before any HTTP response");
        let mut cause: Option<&(dyn Error + 'static)> = Some(&error);
        let mut certificate_error = false;
        while let Some(current) = cause {
            certificate_error |= current
                .to_string()
                .to_ascii_lowercase()
                .contains("invalid peer certificate");
            cause = current.source();
        }
        assert!(
            certificate_error,
            "{endpoint} must fail certificate verification, not connectivity: {error:?}"
        );
        println!("{endpoint} rejected: {error:?}");
    }
}
