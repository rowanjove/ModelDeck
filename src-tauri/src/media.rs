pub const MAX_IMAGE_BYTES: usize = 15 * 1024 * 1024;
pub const MAX_VIDEO_BYTES: usize = 80 * 1024 * 1024;
pub const MAX_VIDEO_PREVIEW_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_AUDIO_BYTES: usize = 40 * 1024 * 1024;
pub const MAX_AUDIO_PREVIEW_BYTES: usize = 20 * 1024 * 1024;

pub fn sniff_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("image/png");
    }
    if bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    None
}

pub fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 24 || sniff_mime(bytes) != Some("image/png") || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    if width == 0 || height == 0 {
        return None;
    }
    Some((width, height))
}

pub fn sniff_video(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        return Some("video/mp4");
    }
    if bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return Some("video/webm");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    None
}

pub fn sniff_audio(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"ID3") {
        return Some("audio/mpeg");
    }
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] & 0xE0 == 0xE0 {
        return Some("audio/mpeg");
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE" {
        return Some("audio/wav");
    }
    if bytes.starts_with(b"OggS") {
        return Some("audio/ogg");
    }
    if bytes.starts_with(b"fLaC") {
        return Some("audio/flac");
    }
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" && &bytes[8..12] == b"M4A " {
        return Some("audio/mp4");
    }
    None
}

pub fn extension_for(mime: &str) -> &'static str {
    match mime {
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "video/webm" => "webm",
        "video/mp4" => "mp4",
        "audio/mpeg" => "mp3",
        "audio/wav" => "wav",
        "audio/ogg" => "ogg",
        "audio/flac" => "flac",
        "audio/mp4" => "m4a",
        _ => "png",
    }
}

pub fn describe_audio(bytes: &[u8]) -> Result<String, &'static str> {
    if bytes.len() > MAX_AUDIO_BYTES {
        return Err("音频超过 40MB");
    }
    sniff_audio(bytes)
        .map(ToOwned::to_owned)
        .ok_or("响应不是受支持的音频")
}

pub fn describe_video(bytes: &[u8]) -> Result<String, &'static str> {
    if bytes.len() > MAX_VIDEO_BYTES {
        return Err("视频超过 80MB");
    }
    sniff_video(bytes)
        .map(ToOwned::to_owned)
        .ok_or("响应不是受支持的视频")
}

pub fn describe(bytes: &[u8]) -> Result<(String, Option<u32>, Option<u32>), &'static str> {
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err("图像超过 15MB");
    }
    let mime = sniff_mime(bytes).ok_or("响应不是受支持的图像")?;
    let size = if mime == "image/png" {
        png_size(bytes)
    } else {
        None
    };
    Ok((
        mime.to_string(),
        size.map(|(width, _)| width),
        size.map(|(_, height)| height),
    ))
}
