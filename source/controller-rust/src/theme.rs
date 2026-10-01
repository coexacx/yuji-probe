use crate::core::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use image::{DynamicImage, ImageFormat, ImageReader, Limits};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::BTreeMap, io::Cursor, sync::Arc};
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
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Palette {
    pub light: BTreeMap<String, String>,
    pub dark: BTreeMap<String, String>,
}
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PackageInfo {
    pub id: String,
    pub name: String,
    pub author: String,
    pub version: String,
    pub description: String,
    pub license: String,
}
impl PackageInfo {
    fn validate(&self) -> ApiResult<()> {
        let slug = !self.id.is_empty()
            && self.id.len() <= 48
            && self
                .id
                .bytes()
                .next()
                .is_some_and(|b| b.is_ascii_lowercase())
            && self
                .id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        let version = !self.version.is_empty()
            && self.version.len() <= 32
            && self
                .version
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".+-".contains(&b));
        if !slug
            || !version
            || self.name.trim().is_empty()
            || self.name.chars().count() > 48
            || self.author.chars().count() > 80
            || self.description.chars().count() > 400
            || self.license.len() > 80
            || [&self.name, &self.author, &self.description, &self.license]
                .iter()
                .any(|s| s.chars().any(char::is_control))
        {
            return Err(bad("主题名称、标识、版本或作者信息不正确"));
        }
        Ok(())
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ThemePackage {
    format: String,
    schema_version: u32,
    metadata: PackageInfo,
    theme: Theme,
}
impl ThemePackage {
    fn into_theme(self, revision: &str) -> ApiResult<Theme> {
        if self.format != "yuji-theme" || self.schema_version != 1 {
            return Err(bad("请选择格式版本为 1 的羽迹主题包"));
        }
        self.metadata.validate()?;
        if !self.theme.revision.is_empty() || self.theme.package.is_some() {
            return Err(bad("主题包不能包含运行中的版本或嵌套主题信息"));
        }
        Ok(Theme {
            revision: revision.into(),
            package: Some(self.metadata),
            ..self.theme
        })
    }
}
const PALETTE_TOKENS: &[(&str, &str, bool)] = &[
    ("bg", "--bg", false),
    ("text", "--text", true),
    ("muted", "--muted", true),
    ("quiet", "--quiet", true),
    ("glass", "--glass", false),
    ("glassStrong", "--glass-strong", false),
    ("inset", "--inset", false),
    ("inputBg", "--input-bg", false),
    ("edge", "--edge", false),
    ("line", "--line", false),
    ("accent", "--accent", true),
    ("accentWash", "--accent-wash", false),
    ("track", "--track", false),
    ("dialog", "--dialog", false),
    ("chart", "--chart", true),
    ("focus", "--focus", true),
    ("terminalSurface", "--terminal-surface", false),
    ("buttonColor", "--theme-button-color", true),
    ("buttonText", "--theme-button-text", true),
];
impl Palette {
    fn stylesheet(&self) -> ApiResult<String> {
        let mut result = String::new();
        for (mode, values) in [("light", &self.light), ("dark", &self.dark)] {
            if values.len() > PALETTE_TOKENS.len() {
                return Err(bad("主题配色项过多"));
            }
            let mut declarations = String::new();
            for (key, value) in values {
                let Some((_, property, opaque)) =
                    PALETTE_TOKENS.iter().find(|(name, _, _)| *name == key)
                else {
                    return Err(bad("主题使用了不支持的配色项"));
                };
                if !color(value, if *opaque { &[3, 6] } else { &[3, 6, 8] })
                    && (*opaque || value != "transparent")
                {
                    return Err(bad("主题配色必须使用十六进制颜色，文字颜色不可透明"));
                }
                declarations.push_str(&format!("{property}:{value};"));
            }
            if !declarations.is_empty() {
                result.push_str(&format!(
                    "html[data-appearance][data-theme={mode}]{{{declarations}}}\n"
                ));
            }
        }
        Ok(result)
    }
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
    pub palette: Palette,
    pub package: Option<PackageInfo>,
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
            palette: Palette::default(),
            package: None,
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
    pub fn migrate_removed(&mut self) -> bool {
        if self.preset != "alpine" {
            return false;
        }
        self.preset = "seasons".into();
        self.revision = token()[..32].into();
        true
    }
    fn stylesheet(&self) -> ApiResult<String> {
        Ok(self.palette.stylesheet()? + &css(&self.custom_css)?)
    }
    pub fn fields(&self) -> ApiResult<()> {
        if !["default", "clear", "sketch", "anime", "seasons"].contains(&self.preset.as_str()) {
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
        self.palette.stylesheet()?;
        if let Some(metadata) = &self.package {
            metadata.validate()?;
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
        let value = json!({"theme":i.data.theme,"css":i.data.theme.stylesheet()?});
        let raw = serde_json::to_vec(&value).map_err(|_| ApiError::internal())?;
        i.theme_cache = Some((i.data.theme.revision.clone(), Arc::from(raw)));
    }
    let mut reply = ApiReply::ok(serde_json::Value::Null);
    reply.raw = Some(i.theme_cache.as_ref().unwrap().1.clone());
    Ok(reply)
}
pub async fn change(app: App, c: Context, body: Vec<u8>) -> ApiResult<ApiReply> {
    let preview = c.path == "/api/admin/theme/preview" && c.method == "POST";
    let importing = c.path == "/api/admin/theme/import" && c.method == "POST";
    if !importing && !preview && !(c.path == "/api/admin/theme" && c.method == "PUT") {
        return Err(ApiError::new(405, "请求方法不正确"));
    }
    let old = {
        let mut i = app.lock();
        app.guard(&mut i, &c, true, true)?;
        i.data.theme.clone()
    };
    let input: Theme = if importing {
        decode::<ThemePackage>(&body)?.into_theme(&old.revision)?
    } else {
        decode(&body)?
    };
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
    let rules = value.stylesheet()?;
    if !preview && !importing {
        value.revision = token()[..32].into();
        let mut data = i.data.clone();
        data.theme = value.clone();
        app.save_data(&mut i, data)?;
        i.theme_cache = None;
        app.record(&mut i, "theme_updated", &value.preset);
    }
    Ok(ApiReply::ok(json!({"theme":value,"css":rules})))
}
pub fn export(app: &App, i: &mut Inner, c: &Context) -> ApiResult<ApiReply> {
    app.guard(i, c, true, false)?;
    let mut theme = i.data.theme.clone();
    let metadata = theme.package.take().unwrap_or(PackageInfo {
        id: "custom-theme".into(),
        name: "自定义主题".into(),
        version: "1.0.0".into(),
        ..Default::default()
    });
    theme.revision.clear();
    let mut visual = serde_json::to_value(theme).map_err(|_| ApiError::internal())?;
    if let Some(fields) = visual.as_object_mut() {
        fields.remove("revision");
        fields.remove("package");
    }
    Ok(ApiReply::ok(
        json!({"format":"yuji-theme","schemaVersion":1,"metadata":metadata,"theme":visual}),
    ))
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
    #[test]
    fn package_schema_and_runtime_fields_are_checked() {
        let base = json!({"format":"yuji-theme","schemaVersion":1,"metadata":{"id":"sample-theme","name":"示例","version":"1.0.0"},"theme":{}});
        let load = |value| {
            serde_json::from_value::<ThemePackage>(value)
                .ok()
                .and_then(|p| p.into_theme("12345678901234567890123456789012").ok())
        };
        let theme = load(base.clone()).unwrap();
        assert_eq!(theme.revision, "12345678901234567890123456789012");
        assert_eq!(theme.package.unwrap().id, "sample-theme");
        for (pointer, value) in [
            ("/format", json!("html")),
            ("/schemaVersion", json!(2)),
            ("/metadata/id", json!("../../auth")),
            ("/metadata/id", json!("x;body")),
            ("/metadata/version", json!("../config")),
            ("/metadata/name", json!("")),
        ] {
            let mut bad = base.clone();
            *bad.pointer_mut(pointer).unwrap() = value;
            assert!(load(bad).is_none(), "{pointer}");
        }
        for (key, value) in [
            ("revision", json!("12345678901234567890123456789012")),
            ("package", base["metadata"].clone()),
            ("script", json!("alert(1)")),
        ] {
            let mut bad = base.clone();
            bad["theme"][key] = value;
            assert!(load(bad).is_none(), "{key}");
        }
        let mut bad = base.clone();
        bad["files"] = json!({"../auth.json":"overwritten"});
        assert!(load(bad).is_none());
    }
    #[test]
    fn palette_rejects_css_and_unsafe_tokens() {
        let mut p = Palette::default();
        p.light.insert("text".into(), "#234".into());
        p.dark.insert("glass".into(), "#12345678".into());
        let css = p.stylesheet().unwrap();
        assert!(css.contains("--text:#234;"));
        assert!(css.contains("[data-theme=dark]{--glass:#12345678;"));
        for value in [
            "transparent",
            "#ffffff00",
            "red",
            "var(--x)",
            "url(https://evil.invalid)",
            "#fff;}body{display:none",
            "</style>",
        ] {
            p.light.insert("text".into(), value.into());
            assert!(p.stylesheet().is_err(), "{value}");
        }
        p.light.clear();
        p.light.insert("credentialURL".into(), "#fff".into());
        assert!(p.stylesheet().is_err());
        p.light.clear();
        p.light.insert("glass".into(), "transparent".into());
        assert!(p.stylesheet().is_ok());
    }
    #[test]
    fn removed_theme_migration_is_idempotent_and_retains_visual_settings() {
        let mut old = Theme {
            preset: "alpine".into(),
            accent: "#336655".into(),
            background_dim: 35,
            ..Default::default()
        };
        assert!(old.fields().is_err());
        assert!(old.migrate_removed());
        assert_eq!(old.preset, "seasons");
        assert_eq!(old.accent, "#336655");
        assert_eq!(old.background_dim, 35);
        assert_eq!(old.revision.len(), 32);
        assert!(old.validate().is_ok());
        let revision = old.revision.clone();
        assert!(!old.migrate_removed());
        assert_eq!(old.revision, revision);
    }
    #[test]
    fn distributed_example_is_valid_and_can_round_trip() {
        let package: ThemePackage = serde_json::from_str(include_str!(
            "../../../docs/themes/waterside.yuji-theme.json"
        ))
        .unwrap();
        let theme = package
            .into_theme("")
            .unwrap()
            .normalize(&Theme::default())
            .unwrap();
        assert!(theme.validate().is_ok());
        let restored: Theme = serde_json::from_slice(&serde_json::to_vec(&theme).unwrap()).unwrap();
        assert!(restored.validate().is_ok());
        assert_eq!(restored.stylesheet().unwrap(), theme.stylesheet().unwrap());
    }
}
