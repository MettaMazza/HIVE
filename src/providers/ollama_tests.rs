    use super::*;
    use crate::models::scope::Scope;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn test_provider_success() {
        let mock_server = MockServer::start().await;
        
        let mut provider = OllamaProvider::new();
        provider.endpoint = format!("{}/api/chat", mock_server.uri());

        let mock_response = "{\"message\": {\"role\": \"assistant\", \"content\": \"Sure, here's your context.\"}, \"done\": true}\n";

        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_string(mock_response))
            .mount(&mock_server)
            .await;

        let history = vec![
            Event { platform: "cli".into(), scope: Scope::Public { channel_id: "t".into(), user_id: "t".into() }, author_name: "Apis".into(), author_id: "test".into(), content: "I am here.".into(), timestamp: None, message_index: None },
            Event { platform: "cli".into(), scope: Scope::Public { channel_id: "t".into(), user_id: "t".into() }, author_name: "Alice".into(), author_id: "test".into(), content: "Hi!".into(), timestamp: None, message_index: None },
        ];
        
        // Single JSON response is technically a 1-line stream chunk
        let new_event = Event {
            platform: "cli".into(),
            scope: Scope::Public { channel_id: "t".into(), user_id: "t".into() },
            author_name: "Bob".into(),
            author_id: "test".into(),
            content: "What's up?".into(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        };
        let res = provider.generate("sys", &history, &new_event, "", None, None).await.unwrap();

        assert_eq!(res, "Sure, here's your context.");
    }

    #[tokio::test]
    async fn test_provider_http_error() {
        let mock_server = MockServer::start().await;
        
        let mut provider = OllamaProvider::new();
        provider.endpoint = format!("{}/api/chat", mock_server.uri());

        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
            .mount(&mock_server)
            .await;

        let res = provider.generate("sys", &[], &Event {
            platform: "cli".into(),
            scope: Scope::Public { channel_id: "t".into(), user_id: "t".into() },
            author_name: "Bob".into(),
            author_id: "test".into(),
            content: "Bork?".into(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        }, "", None, None).await;

        assert!(matches!(res, Err(ProviderError::ParseError(_))));
    }

    #[tokio::test]
    async fn test_provider_connection_error() {
        let mut provider = OllamaProvider::new();
        provider.endpoint = "http://invalid.domain.that.does.not.exist:1234/api/chat".into();

        let res = provider.generate("sys", &[], &Event {
            platform: "cli".into(),
            scope: Scope::Public { channel_id: "t".into(), user_id: "t".into() },
            author_name: "Bob".into(),
            author_id: "test".into(),
            content: "Bork?".into(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        }, "", None, None).await;

        assert!(matches!(res, Err(ProviderError::ConnectionError(_))));
    }

    #[tokio::test]
    async fn test_provider_parse_error() {
        let mock_server = MockServer::start().await;
        let mut provider = OllamaProvider::new();
        provider.endpoint = format!("{}/api/chat", mock_server.uri());

        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_string("invalid json body!\n"))
            .mount(&mock_server)
            .await;

        let res = provider.generate("sys", &[], &Event {
            platform: "cli".into(),
            scope: Scope::Public { channel_id: "t".into(), user_id: "t".into() },
            author_name: "Bob".into(),
            author_id: "test".into(),
            content: "Bork?".into(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        }, "", None, None).await;

        assert!(matches!(res, Err(ProviderError::ParseError(_))));
    }

    #[tokio::test]
    async fn test_provider_early_eof() {
        let mock_server = MockServer::start().await;
        let mut provider = OllamaProvider::new();
        provider.endpoint = format!("{}/api/chat", mock_server.uri());

        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_string(""))
            .mount(&mock_server)
            .await;

        // No chunks, natural EOF. 
        let res = provider.generate("sys", &[], &Event {
            platform: "cli".into(),
            scope: Scope::Public { channel_id: "t".into(), user_id: "t".into() },
            author_name: "Bob".into(),
            author_id: "test".into(),
            content: "Bork?".into(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        }, "", None, None).await;

        assert_eq!(res.unwrap(), "");
    }

    #[tokio::test]
    async fn test_provider_reasoning_telemetry() {
        let mock_server = MockServer::start().await;
        let mut provider = OllamaProvider::new();
        provider.endpoint = format!("{}/api/chat", mock_server.uri());

        let mock_response = "{\"message\": {\"role\": \"assistant\", \"thinking\": \"I am thinking...\", \"content\": \"Final answer\"}, \"done\": true}\n";

        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_string(mock_response))
            .mount(&mock_server)
            .await;

        let (tx, mut rx) = mpsc::channel(10);
        
        let res = provider.generate("sys", &[], &Event {
            platform: "cli".into(),
            scope: Scope::Public { channel_id: "t".into(), user_id: "t".into() },
            author_name: "Bob".into(),
            author_id: "test".into(),
            content: "Bork?".into(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        }, "", Some(tx), None).await;

        let first_recv = rx.recv().await.unwrap();
        assert_eq!(first_recv, "I am thinking...");
        assert_eq!(res.unwrap(), "Final answer");
    }

    #[tokio::test]
    async fn test_provider_missing_content() {
        let mock_server = MockServer::start().await;
        let mut provider = OllamaProvider::new();
        provider.endpoint = format!("{}/api/chat", mock_server.uri());

        let mock_response = "{\"message\": {\"role\": \"assistant\"}, \"done\": true}\n";

        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_string(mock_response))
            .mount(&mock_server)
            .await;

        let res = provider.generate("sys", &[], &Event {
            platform: "cli".into(),
            scope: Scope::Public { channel_id: "t".into(), user_id: "t".into() },
            author_name: "Bob".into(),
            author_id: "test".into(),
            content: "Bork?".into(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        }, "", None, None).await;

        assert_eq!(res.unwrap(), "");
    }

    #[tokio::test]
    async fn test_ollama_stream_fragmented() {
        let mock_server = MockServer::start().await;
        let mut provider = OllamaProvider::new();
        provider.endpoint = format!("{}/api/chat", mock_server.uri());

        let mock_response = "{\"message\": {\"role\": \"assistant\", \"content\": \"part1\"}}\n{\"message\": {\"content\": \" part2\"}}\n{\"message\": {\"content\": \" done!\"}, \"done\": true}\n";

        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_string(mock_response))
            .mount(&mock_server)
            .await;

        let res = provider.generate("sys", &[], &Event {
            platform: "cli".into(),
            scope: Scope::Public { channel_id: "t".into(), user_id: "t".into() },
            author_name: "Bob".into(),
            author_id: "test".into(),
            content: "Stream?".into(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        }, "", None, None).await;

        assert_eq!(res.unwrap(), "part1 part2 done!");
    }

    #[tokio::test]
    async fn test_ollama_stream_disconnect() {
        let mock_server = MockServer::start().await;
        let mut provider = OllamaProvider::new();
        provider.endpoint = format!("{}/api/chat", mock_server.uri());

        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(503).set_body_string("Service Unavailable Drops Stream"))
            .mount(&mock_server)
            .await;

        let res = provider.generate("sys", &[], &Event {
            platform: "cli".into(),
            scope: Scope::Public { channel_id: "t".into(), user_id: "t".into() },
            author_name: "Bob".into(),
            author_id: "test".into(),
            content: "Disconnect?".into(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        }, "", None, None).await;

        assert!(res.is_err());
    }
