use crate::core::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use image::{DynamicImage, ImageFormat, ImageReader, Limits};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{io::Cursor, sync::Arc};
use tokio::sync::Semaphore;

pub const BODY_LIMIT: usize = 3 * 1024 * 1024;
const BACKGROUND_LIMIT: usize = 1536 * 1024;
const ICON_LIMIT: usize = 192 * 1024;
static IMAGE_WORK: Semaphore = Semaphore::const_new(1);

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ThemeImage {
    pub mime: String,
    pub data: String,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct Theme {
    pub revision: String,
    pub preset: String,
    pub accent: String,
    pub background: Option<ThemeImage>,
    pub favicon: Option<ThemeImage>,
    pub background_dim: u8,
    pub background_blur: u8,
    pub brand_icon: bool,
    pub custom_css: String,
}
impl Default for Theme {
    fn default() -> Self {
        Self {
            revision: String::new(),
            preset: "default".into(),
            accent: String::new(),
            background: None,
            favicon: None,
            background_dim: 20,
            background_blur: 0,
            brand_icon: true,
            custom_css: String::new(),
        }
    }
}
fn bad(message: &str) -> ApiError {
    ApiError::new(400, message)
}
fn color(s: &str, sizes: &[usize]) -> bool {
    s.strip_prefix('#')
        .is_some_and(|s| sizes.contains(&s.len()) && s.bytes().all(|b| b.is_ascii_hexdigit()))
}
fn pixels(s: &str, min: u32, max: u32) -> bool {
    let Some(n) = s.strip_suffix("px") else {
        return s == "0" && min == 0;
    };
    !n.is_empty()
        && n.len() <= 3
        && n.bytes().all(|c| c.is_ascii_digit())
        && n.parse::<u32>().is_ok_and(|v| (min..=max).contains(&v))
}
pub fn css(input: &str) -> ApiResult<String> {
    if input.len() > 4096
        || input
            .chars()
            .any(|c| !c.is_ascii() || (c.is_ascii_control() && !"\n\r\t".contains(c)))
    {
        return Err(bad("局部样式最多 4096 字节，请使用列表中的选择器与属性"));
    }
    let selectors = [
        ".server-card",
        ".node-title",
        ".card-footer",
        ".toolbar",
        ".brand-mark",
        ".brand-name",
        ".footer",
        ".admin-box",
        ".node-dialog",
    ];
    let mut rest = input.trim();
    let mut result = String::new();
    let mut rules = 0;
    while !rest.is_empty() {
        rules += 1;
        if rules > 16 {
            return Err(bad("局部样式最多 16 组规则"));
        }
        let (head, body) = rest.split_once('{').ok_or_else(|| bad("局部样式缺少 {"))?;
        let (body, tail) = body.split_once('}').ok_or_else(|| bad("局部样式缺少 }"))?;
        let selector = head.trim();
        if !selectors.contains(&selector) {
            return Err(bad("局部样式的选择器不在支持列表中"));
        }
        let scoped = if selector == ".node-dialog" {
            "html[data-appearance] .node-dialog:not(.terminal-dialog)".to_string()
        } else {
            format!("html[data-appearance] {selector}")
        };
        let mut declarations = Vec::new();
        for pair in body.split(';').map(str::trim).filter(|s| !s.is_empty()) {
            let (property, value) = pair
                .split_once(':')
                .ok_or_else(|| bad("局部样式属性格式不正确"))?;
            let property = property.trim();
            let value = value.trim();
            let valid = match property {
                "color" | "background-color" | "border-color" => {
                    color(value, &[3, 6, 8]) || value == "transparent"
                }
                "border-radius" => {
                    value.split_whitespace().count() <= 4
                        && !value.is_empty()
                        && value.split_whitespace().all(|s| pixels(s, 0, 40))
                }
                "border-width" => pixels(value, 0, 3),
                "font-size" => pixels(value, 11, 32),
                "font-weight" => ["400", "500", "600", "700"].contains(&value),
                "letter-spacing" => pixels(value, 0, 4),
                _ => false,
            };
            if !valid {
                return Err(bad(
                    "局部样式仅支持颜色、圆角、边框宽度、字号、字重和字距；请检查属性值",
                ));
            }
            declarations.push(format!("{property}:{value}"));
            if declarations.len() > 12 {
                return Err(bad("每组局部样式最多 12 项属性"));
            }
        }
        if declarations.is_empty() {
            return Err(bad("局部样式不能是空规则"));
        }
        result.push_str(&format!("{scoped}{{{}}}\n", declarations.join(";")));
        rest = tail.trim();
    }
    Ok(result)
}
fn decode_image(value: &ThemeImage, icon: bool) -> ApiResult<DynamicImage> {
    let limit = if icon { ICON_LIMIT } else { BACKGROUND_LIMIT };
    if value.data.len() > limit.div_ceil(3) * 4 {
        return Err(bad("图片文件过大，请降低尺寸后上传"));
    }
    let raw = STANDARD
        .decode(&value.data)
        .map_err(|_| bad("图片编码不正确"))?;
    if raw.len() > limit {
        return Err(bad("图片文件过大"));
    }
    let reader = ImageReader::new(Cursor::new(&raw))
        .with_guessed_format()
        .map_err(|_| bad("无法读取图片"))?;
    let mime = match reader.format() {
        Some(ImageFormat::Png) => "image/png",
        Some(ImageFormat::Jpeg) => "image/jpeg",
        Some(ImageFormat::WebP) => "image/webp",
        _ => return Err(bad("请上传 PNG、JPG 或 WebP 图片")),
    };
    if mime != value.mime {
        return Err(bad("图片类型与内容不匹配"));
    }
    let mut reader = reader;
    let mut limits = Limits::default();
    limits.max_image_width = Some(if icon { 1024 } else { 4096 });
    limits.max_image_height = limits.max_image_width;
    limits.max_alloc = Some(80 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().map_err(|_| bad("图片损坏或尺寸超出限制"))?;
    if image.width() == 0
        || image.height() == 0
        || u64::from(image.width()) * u64::from(image.height()) > 16_777_216
    {
        return Err(bad("图片尺寸超出限制"));
    }
    Ok(image)
}
fn normalize_image(value: &ThemeImage, icon: bool) -> ApiResult<ThemeImage> {
    let image = decode_image(value, icon)?;
    let edge = if icon { 192 } else { 1920 };
    let image = if image.width() > edge || image.height() > edge {
        image.thumbnail(edge, edge)
    } else {
        image
    };
    let mut raw = Vec::new();
    let mime = if icon {
        image
            .write_to(&mut Cursor::new(&mut raw), ImageFormat::Png)
            .map_err(|_| bad("站点图标转换失败"))?;
        "image/png"
    } else {
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut raw, 85)
            .encode_image(&DynamicImage::ImageRgb8(image.to_rgb8()))
            .map_err(|_| bad("背景图转换失败"))?;
        "image/jpeg"
    };
    if raw.len() > if icon { ICON_LIMIT } else { BACKGROUND_LIMIT } {
        return Err(bad("转换后的图片过大，请选择更小的图片"));
    }
    Ok(ThemeImage {
        mime: mime.into(),
        data: STANDARD.encode(raw),
    })
}
impl Theme {
    pub fn fields(&self) -> ApiResult<()> {
        if !["default", "clear", "sketch", "anime", "seasons", "alpine"]
            .contains(&self.preset.as_str())
        {
            return Err(bad("请选择可用主题"));
        }
        if !(self.accent.is_empty() || color(&self.accent, &[6])) {
            return Err(bad("主题色需为六位十六进制颜色"));
        }
        if self.background_dim > 85 || self.background_blur > 24 {
            return Err(bad("背景参数超出范围"));
        }
        if !(self.revision.is_empty()
            || (self.revision.len() == 32 && self.revision.bytes().all(|c| c.is_ascii_hexdigit())))
        {
            return Err(bad("主题版本不正确"));
        }
        css(&self.custom_css)?;
        Ok(())
    }
    pub fn validate(&self) -> ApiResult<()> {
        self.fields()?;
        if let Some(v) = &self.background {
            decode_image(v, false)?;
        }
        if let Some(v) = &self.favicon {
            decode_image(v, true)?;
        }
        Ok(())
    }
    fn normalize(mut self, old: &Self) -> ApiResult<Self> {
        self.fields()?;
        self.accent = self.accent.to_ascii_lowercase();
        if self.background != old.background
            && let Some(v) = &self.background
        {
            self.background = Some(normalize_image(v, false)?);
        }
        if self.favicon != old.favicon
            && let Some(v) = &self.favicon
        {
            self.favicon = Some(normalize_image(v, true)?);
        }
        Ok(self)
    }
}
pub fn read(i: &mut Inner) -> ApiResult<ApiReply> {
    if i.theme_cache
        .as_ref()
        .is_none_or(|(revision, _)| revision != &i.data.theme.revision)
    {
        let value = json!({"theme":i.data.theme,"css":css(&i.data.theme.custom_css)?});
        let raw = serde_json::to_vec(&value).map_err(|_| ApiError::internal())?;
        i.theme_cache = Some((i.data.theme.revision.clone(), Arc::from(raw)));
    }
    let mut reply = ApiReply::ok(serde_json::Value::Null);
    reply.raw = Some(i.theme_cache.as_ref().unwrap().1.clone());
    Ok(reply)
}
pub async fn change(app: App, c: Context, body: Vec<u8>) -> ApiResult<ApiReply> {
    let preview = c.path == "/api/admin/theme/preview" && c.method == "POST";
    if !preview && !(c.path == "/api/admin/theme" && c.method == "PUT") {
        return Err(ApiError::new(405, "请求方法不正确"));
    }
    let old = {
        let mut i = app.lock();
        app.guard(&mut i, &c, true, true)?;
        i.data.theme.clone()
    };
    let input: Theme = decode(&body)?;
    if input.revision != old.revision {
        return Err(ApiError::new(
            409,
            "主题已被其他页面修改，请重新打开主题设置",
        ));
    }
    let permit = IMAGE_WORK
        .try_acquire()
        .map_err(|_| ApiError::new(429, "正在处理另一张图片，请稍后重试"))?;
    let mut value = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        input.normalize(&old)
    })
    .await
    .map_err(|_| ApiError::internal())??;
    let mut i = app.lock();
    app.guard(&mut i, &c, true, true)?;
    if value.revision != i.data.theme.revision {
        return Err(ApiError::new(409, "主题已更新，请重新打开主题设置"));
    }
    let rules = css(&value.custom_css)?;
    if !preview {
        value.revision = token()[..32].into();
        let mut data = i.data.clone();
        data.theme = value.clone();
        app.save_data(&mut i, data)?;
        i.theme_cache = None;
        app.record(&mut i, "theme_updated", &value.preset);
    }
    Ok(ApiReply::ok(json!({"theme":value,"css":rules})))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn css_only_allows_bounded_visual_rules() {
        assert!(
            css(".server-card { border-radius: 18px; border-color: #AbC; }")
                .unwrap()
                .starts_with("html[data-appearance] .server-card")
        );
        for invalid in [
            "@import 'https://example.com/a.css';",
            ".server-card{background:url(https://evil.example)}",
            ".server-card{color:var(--stolen)}",
            ".server-card{color:expression(1)}",
            ".server-card{position:fixed}",
            ".server-card{font-size:0px}",
            ".server-card{color:red!important}",
            ".server-card{color:#fff} body{display:none}",
            ".server-card{color:#fff;--x:1}",
            ".server-card{color:#fff}</style><script>alert(1)</script>",
            ".server-card{color:\\23fff}",
            "[value^=a]{color:#fff}",
            ".node-dialog{opacity:0}",
            ".server-card{border-radius:99px}",
        ] {
            assert!(css(invalid).is_err(), "{invalid}");
        }
        assert!(css("").unwrap().is_empty());
    }
    #[test]
    fn defaults_and_theme_bounds() {
        let t: Theme = serde_json::from_str("{}").unwrap();
        assert_eq!(t.preset, "default");
        assert!(t.validate().is_ok());
        for t in [
            Theme {
                preset: "bad".into(),
                ..Default::default()
            },
            Theme {
                accent: "red".into(),
                ..Default::default()
            },
            Theme {
                background_dim: 86,
                ..Default::default()
            },
            Theme {
                background_blur: 25,
                ..Default::default()
            },
        ] {
            assert!(t.validate().is_err());
        }
        assert!(serde_json::from_str::<Theme>(r#"{"script":"alert(1)"}"#).is_err());
    }
    #[test]
    fn uploaded_images_are_decoded_and_reencoded() {
        let mut raw = Cursor::new(Vec::new());
        DynamicImage::new_rgba8(8, 8)
            .write_to(&mut raw, ImageFormat::Png)
            .unwrap();
        raw.get_mut()
            .extend_from_slice(b"<script>discard me</script>");
        let input = ThemeImage {
            mime: "image/png".into(),
            data: STANDARD.encode(raw.into_inner()),
        };
        let output = normalize_image(&input, true).unwrap();
        let raw = STANDARD.decode(&output.data).unwrap();
        assert!(!raw.windows(7).any(|v| v == b"<script"));
        assert_eq!(decode_image(&output, true).unwrap().width(), 8);
        for mime in ["image/svg+xml", "text/html", "image/jpeg"] {
            assert!(
                decode_image(
                    &ThemeImage {
                        mime: mime.into(),
                        ..input.clone()
                    },
                    true
                )
                .is_err()
            );
        }
        assert!(
            decode_image(
                &ThemeImage {
                    mime: "image/png".into(),
                    data: STANDARD.encode(b"<svg onload='alert(1)'/>")
                },
                true
            )
            .is_err()
        );
        assert!(
            decode_image(
                &ThemeImage {
                    mime: "image/png".into(),
                    data: "A".repeat(ICON_LIMIT * 2)
                },
                true
            )
            .is_err()
        );
    }
}
