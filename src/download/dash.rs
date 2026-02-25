use crate::error::{Error, Result};
use std::collections::HashMap;

/// Parsed representation info
#[derive(Debug, Clone)]
pub struct Representation {
    pub id: String,
    pub bandwidth: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub codecs: Option<String>,
    pub mime_type: Option<String>,
    pub base_url: Option<String>,
    pub initialization: Option<String>,
    pub segments: Vec<SegmentInfo>,
}

/// Segment information
#[derive(Debug, Clone)]
pub struct SegmentInfo {
    pub url: String,
    pub duration: Option<f64>,
    pub byte_range: Option<(u64, u64)>,
}

/// DASH MPD parser (simplified implementation)
pub struct DashParser {
    base_url: String,
}

impl DashParser {
    pub fn new(base_url: &str) -> Self {
        // Extract base URL from full URL
        let base = if let Some(pos) = base_url.rfind('/') {
            &base_url[..=pos]
        } else {
            base_url
        };
        Self {
            base_url: base.to_string(),
        }
    }

    /// Parse MPD content using dash-mpd crate
    pub fn parse(&self, content: &str) -> Result<ParsedMpd> {
        let mpd = dash_mpd::parse(content)
            .map_err(|e| Error::ManifestParse(format!("Failed to parse MPD: {}", e)))?;

        let mut video_representations = Vec::new();
        let mut audio_representations = Vec::new();
        let mut subtitles = HashMap::new();
        let mut pssh: Option<String> = None;

        tracing::debug!("MPD has {} periods", mpd.periods.len());

        // Process periods
        for period in &mpd.periods {
            tracing::debug!("Period has {} adaptation sets", period.adaptations.len());
            // Process adaptation sets
            for adaptation_set in &period.adaptations {
                let as_content_type = adaptation_set.contentType.as_deref();
                let as_mime_type = adaptation_set.mimeType.as_deref();
                let as_codecs = adaptation_set.codecs.as_deref();

                tracing::debug!(
                    "AdaptationSet: contentType={:?}, mimeType={:?}, codecs={:?}, reps={}",
                    as_content_type, as_mime_type, as_codecs, adaptation_set.representations.len()
                );

                // Extract PSSH from ContentProtection
                for cp in &adaptation_set.ContentProtection {
                    for cenc_pssh in &cp.cenc_pssh {
                        if pssh.is_none() {
                            if let Some(ref content) = cenc_pssh.content {
                                pssh = Some(content.clone());
                            }
                        }
                    }
                }

                // Process representations
                for rep in &adaptation_set.representations {
                    let id = rep.id.clone().unwrap_or_else(|| "default".to_string());
                    let bandwidth = rep.bandwidth.unwrap_or(0);
                    let width = rep.width.map(|w| w as u32);
                    let height = rep.height.map(|h| h as u32);
                    let rep_codecs = rep.codecs.clone().or_else(|| adaptation_set.codecs.clone());
                    let rep_mime_type = rep.mimeType.clone().or_else(|| adaptation_set.mimeType.clone());

                    // Determine content type from multiple sources
                    // Check AdaptationSet level first, then Representation level
                    let content_type = as_content_type
                        .or(rep.contentType.as_deref());
                    let mime_type = as_mime_type
                        .or(rep.mimeType.as_deref());
                    let codecs = as_codecs
                        .or(rep.codecs.as_deref());

                    tracing::debug!(
                        "  Rep {}: contentType={:?}, mimeType={:?}, codecs={:?}, {}x{:?}",
                        id, content_type, mime_type, codecs, width.unwrap_or(0), height
                    );

                    // Determine if this is video, audio, or text
                    // Check multiple indicators since some manifests don't have all attributes
                    let is_video = content_type == Some("video")
                        || mime_type.map(|m| m.starts_with("video/")).unwrap_or(false)
                        || codecs.map(|c| c.starts_with("avc") || c.starts_with("hvc") || c.starts_with("hev") || c.starts_with("vp")).unwrap_or(false)
                        || (width.is_some() && height.is_some()); // Has dimensions = video

                    let is_audio = !is_video && (
                        content_type == Some("audio")
                        || mime_type.map(|m| m.starts_with("audio/")).unwrap_or(false)
                        || codecs.map(|c| c.starts_with("mp4a") || c.starts_with("ac-") || c.starts_with("ec-") || c.starts_with("opus")).unwrap_or(false)
                        || (width.is_none() && height.is_none() && bandwidth > 0) // No dimensions but has bandwidth = likely audio
                    );

                    let is_text = content_type == Some("text")
                        || mime_type.map(|m| m.starts_with("text/") || m.starts_with("application/ttml")).unwrap_or(false);

                    // Get base URL - hierarchical: Representation > AdaptationSet > Period > MPD
                    let rep_base_url = rep
                        .BaseURL
                        .first()
                        .or_else(|| adaptation_set.BaseURL.first())
                        .or_else(|| period.BaseURL.first())
                        .map(|b| self.resolve_url(&b.base));

                    // Parse segments with the correct base URL
                    let segments = self.parse_segments_from_rep(rep, adaptation_set, rep_base_url.as_deref());

                    // Get initialization segment URL - use rep's BaseURL if available
                    let initialization = rep
                        .SegmentTemplate
                        .as_ref()
                        .or(adaptation_set.SegmentTemplate.as_ref())
                        .and_then(|t| t.initialization.clone())
                        .map(|init| self.resolve_template_with_base(&init, &id, bandwidth, 0, 0, rep_base_url.as_deref()));

                    let representation = Representation {
                        id,
                        bandwidth,
                        width,
                        height,
                        codecs: rep_codecs,
                        mime_type: rep_mime_type,
                        base_url: rep_base_url.clone(),
                        initialization,
                        segments,
                    };

                    tracing::debug!("    -> is_video={}, is_audio={}, is_text={}", is_video, is_audio, is_text);

                    if is_video {
                        video_representations.push(representation);
                    } else if is_audio {
                        audio_representations.push(representation);
                    } else if is_text {
                        if let Some(lang) = &adaptation_set.lang {
                            if let Some(ref url) = representation.base_url {
                                subtitles.insert(lang.clone(), url.clone());
                            }
                        }
                    }
                }
            }
        }

        // Sort by quality (bandwidth/resolution)
        video_representations.sort_by(|a, b| b.bandwidth.cmp(&a.bandwidth));
        audio_representations.sort_by(|a, b| b.bandwidth.cmp(&a.bandwidth));

        Ok(ParsedMpd {
            video_representations,
            audio_representations,
            subtitles,
            pssh,
            duration: mpd.mediaPresentationDuration.map(|d| d.as_secs_f64()),
        })
    }

    fn parse_segments_from_rep(
        &self,
        rep: &dash_mpd::Representation,
        adaptation_set: &dash_mpd::AdaptationSet,
        rep_base_url: Option<&str>,
    ) -> Vec<SegmentInfo> {
        let mut segments = Vec::new();

        // Try SegmentTemplate first
        if let Some(template) = rep.SegmentTemplate.as_ref().or(adaptation_set.SegmentTemplate.as_ref()) {
            if let Some(ref timeline) = template.SegmentTimeline {
                // SegmentTimeline-based
                let mut time = 0u64;
                let mut number = template.startNumber.unwrap_or(1) as u64;
                let timescale = template.timescale.unwrap_or(1) as f64;

                for s in &timeline.segments {
                    let duration = s.d;
                    let start_time = s.t.unwrap_or(time);
                    time = start_time;

                    let repeat = s.r.unwrap_or(0) + 1;
                    for _ in 0..repeat {
                        if let Some(ref media) = template.media {
                            let url = self.resolve_template_with_base(
                                media,
                                &rep.id.clone().unwrap_or_default(),
                                rep.bandwidth.unwrap_or(0),
                                number,
                                time,
                                rep_base_url,
                            );
                            segments.push(SegmentInfo {
                                url,
                                duration: Some(duration as f64 / timescale),
                                byte_range: None,
                            });
                        }
                        time += duration;
                        number += 1;
                    }
                }
            } else if let Some(duration) = template.duration {
                // Duration-based
                let timescale = template.timescale.unwrap_or(1);
                let start_number = template.startNumber.unwrap_or(1);
                let duration_u64 = duration as u64;

                // Generate a reasonable number of segments
                for i in 0..100u64 {
                    let number = start_number as u64 + i;
                    if let Some(ref media) = template.media {
                        let url = self.resolve_template_with_base(
                            media,
                            &rep.id.clone().unwrap_or_default(),
                            rep.bandwidth.unwrap_or(0),
                            number,
                            i * duration_u64,
                            rep_base_url,
                        );
                        segments.push(SegmentInfo {
                            url,
                            duration: Some(duration / timescale as f64),
                            byte_range: None,
                        });
                    }
                }
            }
        }

        // Try SegmentList
        if segments.is_empty() {
            if let Some(ref segment_list) = rep.SegmentList {
                for seg_url in &segment_list.segment_urls {
                    if let Some(ref media) = seg_url.media {
                        segments.push(SegmentInfo {
                            url: self.resolve_url_with_base(media, rep_base_url),
                            duration: None,
                            byte_range: seg_url.mediaRange.as_ref().and_then(|r| {
                                let parts: Vec<&str> = r.split('-').collect();
                                if parts.len() == 2 {
                                    let start: u64 = parts[0].parse().ok()?;
                                    let end: u64 = parts[1].parse().ok()?;
                                    Some((start, end))
                                } else {
                                    None
                                }
                            }),
                        });
                    }
                }
            }
        }

        // Try BaseURL with SegmentBase
        if segments.is_empty() {
            if let Some(base_url) = rep.BaseURL.first() {
                segments.push(SegmentInfo {
                    url: self.resolve_url(&base_url.base),
                    duration: None,
                    byte_range: None,
                });
            }
        }

        segments
    }

    fn resolve_url(&self, url: &str) -> String {
        if url.starts_with("http://") || url.starts_with("https://") {
            url.to_string()
        } else {
            format!("{}{}", self.base_url, url)
        }
    }

    /// Resolve URL with optional custom base URL (for Representation-specific BaseURL)
    fn resolve_url_with_base(&self, url: &str, custom_base: Option<&str>) -> String {
        if url.starts_with("http://") || url.starts_with("https://") {
            url.to_string()
        } else if let Some(base) = custom_base {
            format!("{}{}", base, url)
        } else {
            format!("{}{}", self.base_url, url)
        }
    }

    fn resolve_template_with_base(
        &self,
        template: &str,
        rep_id: &str,
        bandwidth: u64,
        number: u64,
        time: u64,
        custom_base: Option<&str>,
    ) -> String {
        let url = template
            .replace("$RepresentationID$", rep_id)
            .replace("$Bandwidth$", &bandwidth.to_string())
            .replace("$Number$", &number.to_string())
            .replace("$Time$", &time.to_string());

        // Handle format specifiers like $Number%05d$
        let re = regex_lite::Regex::new(r"\$(\w+)%(\d+)d\$").unwrap();
        let url = re.replace_all(&url, |caps: &regex_lite::Captures| {
            let var = &caps[1];
            let width: usize = caps[2].parse().unwrap_or(0);
            let value = match var {
                "Number" => number,
                "Time" => time,
                "Bandwidth" => bandwidth,
                _ => 0,
            };
            format!("{:0width$}", value, width = width)
        });

        self.resolve_url_with_base(&url, custom_base)
    }
}

/// Parsed MPD result
#[derive(Debug, Clone)]
pub struct ParsedMpd {
    pub video_representations: Vec<Representation>,
    pub audio_representations: Vec<Representation>,
    pub subtitles: HashMap<String, String>,
    pub pssh: Option<String>,
    pub duration: Option<f64>,
}

impl ParsedMpd {
    /// Get duration in seconds (with fallback)
    pub fn duration_secs(&self) -> Option<f64> {
        self.duration
    }
}

impl ParsedMpd {
    /// Get best video representation
    pub fn best_video(&self) -> Option<&Representation> {
        self.video_representations.first()
    }

    /// Get best audio representation
    pub fn best_audio(&self) -> Option<&Representation> {
        self.audio_representations.first()
    }

    /// Get video by height (e.g., 1080, 720)
    pub fn video_by_height(&self, height: u32) -> Option<&Representation> {
        self.video_representations
            .iter()
            .find(|r| r.height == Some(height))
            .or_else(|| {
                // Find closest
                self.video_representations
                    .iter()
                    .min_by_key(|r| {
                        r.height
                            .map(|h| (h as i32 - height as i32).abs())
                            .unwrap_or(i32::MAX)
                    })
            })
    }
}
