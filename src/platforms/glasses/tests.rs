    use super::*;

    #[test]
    fn test_glasses_session_audio() {
        let mut session = GlassesSession::new("user1".into(), "User One".into());
        assert!(!session.has_audio());

        // Add 500ms of audio at 16kHz, 16-bit, mono = 16000 bytes
        session.add_audio_chunk(vec![0u8; 16000]);
        assert!(session.has_audio());

        let audio = session.take_audio();
        assert_eq!(audio.len(), 16000);
        assert!(!session.has_audio());
    }

    #[test]
    fn test_glasses_session_frames() {
        let mut session = GlassesSession::new("user1".into(), "User One".into());
        assert!(session.latest_frame().is_none());

        use base64::Engine;
        let test_data = base64::engine::general_purpose::STANDARD.encode(b"test_jpeg_data");
        session.add_frame(&test_data);
        assert!(session.latest_frame().is_some());
        assert_eq!(session.latest_frame().unwrap(), b"test_jpeg_data");

        for i in 0..MAX_FRAME_BUFFER + 1 {
            let data = base64::engine::general_purpose::STANDARD.encode(format!("frame_{}", i).as_bytes());
            session.add_frame(&data);
        }
        assert_eq!(session.frames.len(), MAX_FRAME_BUFFER);
    }

    #[test]
    fn test_validate_token_no_config() {
        let result = GlassesPlatform::validate_token("token=anything");
        assert!(result.is_some() || std::env::var("HIVE_GLASSES_TOKEN").is_ok());
    }

    #[test]
    fn test_platform_name() {
        let platform = GlassesPlatform::new();
        assert_eq!(platform.name(), "glasses");
    }

    #[test]
    fn test_tts_float_to_pcm16_conversion() {
        // Test the float32 → i16 conversion logic
        let sample_f32: f32 = 0.5;
        let clamped = sample_f32.clamp(-1.0, 1.0);
        let sample_i16 = (clamped * 32767.0) as i16;
        assert_eq!(sample_i16, 16383);

        // Full scale positive
        let full_pos = 1.0_f32.clamp(-1.0, 1.0);
        assert_eq!((full_pos * 32767.0) as i16, 32767);

        // Full scale negative
        let full_neg = (-1.0_f32).clamp(-1.0, 1.0);
        assert_eq!((full_neg * 32767.0) as i16, -32767);

        // Over-range clamping
        let over = 1.5_f32.clamp(-1.0, 1.0);
        assert_eq!((over * 32767.0) as i16, 32767);
    }
