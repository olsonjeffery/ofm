use omprint::server;

#[tokio::test]
async fn test_health_endpoint() {
    let app = server::router();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let url = format!("http://{}/health", addr);
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await.unwrap();

    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.unwrap(), "ok");
}
