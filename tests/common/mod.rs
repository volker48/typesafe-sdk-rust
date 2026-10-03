//! Loopback HTTP fixtures shared by the integration tests.
// Each test crate compiles this module and uses a different subset of it.
#![allow(dead_code)]

use std::{collections::BTreeMap, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};
use typesafe_sdk::{Client, Question, RetryPolicy, SystemOneRequest};

/// A minimal valid request.
pub fn request() -> SystemOneRequest {
    SystemOneRequest::new(
        "text".into(),
        BTreeMap::from([("q".into(), Question::noul("Question?"))]),
    )
    .unwrap()
}

/// A client with a fixture key and otherwise default configuration.
pub fn client(base_url: impl Into<String>) -> Client {
    Client::builder()
        .api_key("key")
        .base_url(base_url)
        .build()
        .unwrap()
}

pub fn base_url(listener: &TcpListener) -> String {
    format!("http://{}", listener.local_addr().unwrap())
}

/// Read one complete HTTP/1.1 request, including its body.
pub async fn read_request(stream: &mut TcpStream) -> String {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut bytes = Vec::new();
        let mut buf = [0; 4096];
        loop {
            let n = stream.read(&mut buf).await.unwrap();
            assert!(n > 0, "client closed before completing its request");
            bytes.extend_from_slice(&buf[..n]);
            if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                let length: usize = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length: "))
                    .unwrap_or("0")
                    .parse()
                    .unwrap();
                if bytes.len() == end + 4 + length {
                    return String::from_utf8(bytes).unwrap();
                }
            }
        }
    })
    .await
    .unwrap()
}

/// A complete HTTP/1.1 response. Each header line must end with CRLF.
pub fn response(status: u16, headers: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status} Test\r\nconnection: close\r\ncontent-length: {}\r\n{headers}\r\n{body}",
        body.len()
    )
}

/// Answer one request with `response`. Yields the base URL and, from the
/// task, the raw request received.
pub async fn server(response: String) -> (String, JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = base_url(&listener);
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let request = read_request(&mut stream).await;
        stream.write_all(response.as_bytes()).await.unwrap();
        request
    });
    (url, task)
}

/// Answer `count` sequential connections using `handler`. Yields a client
/// with retries disabled and, from the task, the raw requests received.
pub async fn mock(
    count: usize,
    mut handler: impl FnMut(&str) -> String + Send + 'static,
) -> (Client, JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = Client::builder()
        .api_key("test-key")
        .base_url(base_url(&listener))
        .retry(RetryPolicy::disabled())
        .build()
        .unwrap();
    let task = tokio::spawn(async move {
        let mut requests = Vec::new();
        for _ in 0..count {
            let (mut stream, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
                .await
                .unwrap()
                .unwrap();
            let request = read_request(&mut stream).await;
            stream
                .write_all(handler(&request).as_bytes())
                .await
                .unwrap();
            requests.push(request);
        }
        requests
    });
    (client, task)
}
