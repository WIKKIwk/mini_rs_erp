use super::service::WerkaService;

#[tokio::test]
async fn home_returns_none_without_lookup() {
    let data = WerkaService::new().home(20).await.expect("home result");

    assert!(data.is_none());
}

#[tokio::test]
async fn summary_returns_none_without_lookup() {
    let data = WerkaService::new().summary().await.expect("summary result");

    assert!(data.is_none());
}

#[tokio::test]
async fn pending_returns_none_without_lookup() {
    let data = WerkaService::new()
        .pending(0)
        .await
        .expect("pending result");

    assert!(data.is_none());
}

#[tokio::test]
async fn history_returns_none_without_lookup() {
    let data = WerkaService::new().history().await.expect("history result");

    assert!(data.is_none());
}

#[tokio::test]
async fn status_breakdown_returns_none_without_lookup() {
    let data = WerkaService::new()
        .status_breakdown("pending")
        .await
        .expect("status breakdown result");

    assert!(data.is_none());
}

#[tokio::test]
async fn status_details_returns_none_without_lookup() {
    let data = WerkaService::new()
        .status_details("pending", "SUP-001")
        .await
        .expect("status details result");

    assert!(data.is_none());
}

#[tokio::test]
async fn archive_returns_none_without_lookup() {
    let data = WerkaService::new()
        .archive("sent", "yearly", None, None)
        .await
        .expect("archive result");

    assert!(data.is_none());
}

#[tokio::test]
async fn suppliers_returns_none_without_lookup() {
    let data = WerkaService::new()
        .suppliers("Ali", 20, 3)
        .await
        .expect("suppliers result");

    assert!(data.is_none());
}

#[tokio::test]
async fn customers_returns_none_without_lookup() {
    let data = WerkaService::new()
        .customers("Ali", 20, 3)
        .await
        .expect("customers result");

    assert!(data.is_none());
}

#[tokio::test]
async fn item_searches_return_none_without_lookup() {
    let service = WerkaService::new();

    assert!(
        service
            .supplier_items("SUP-001", "milk", 20, 3)
            .await
            .expect("supplier items result")
            .is_none()
    );
    assert!(
        service
            .customer_items("CUST-001", "milk", 20, 3)
            .await
            .expect("customer items result")
            .is_none()
    );
    assert!(
        service
            .customer_item_options("milk", 20, 3)
            .await
            .expect("customer item options result")
            .is_none()
    );
}
