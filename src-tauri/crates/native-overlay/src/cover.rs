//! A self-contained, versioned source document. No URLs, system paths or GPU
//! are needed to render it. Positions match the editor's 1920×1080 space.
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use skia_safe::{
    image_filters, paint, surfaces,
    textlayout::{
        FontCollection, Paragraph, ParagraphBuilder, ParagraphStyle, TextShadow, TextStyle,
    },
    Canvas, Codec, Color, Data, EncodedImageFormat, FilterMode, FontMgr, FontStyle, Image, Paint,
    Rect,
};
use std::collections::BTreeMap;
use unicode_segmentation::UnicodeSegmentation;

type Result<T> = std::result::Result<T, String>;
const HEIGHT: f32 = 1080.;
const WIDE: i32 = 1920;
const HOME: i32 = 1440;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Pair<T> {
    pub wide: T,
    pub home: T,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Frame {
    x: f32,
    y: f32,
    zoom: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Text {
    content: String,
    font: String,
    font_size: f32,
    bold: bool,
    align: Align,
    #[serde(flatten)]
    ink: Ink,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Ink {
    fill: String,
    stroke: Option<String>,
    stroke_width: f32,
    background: Option<String>,
    shadow: bool,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Align {
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Layer {
    Text(Text),
    Sticker {
        src: String,
        size: f32,
        aspect: f32,
        flip: bool,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Element {
    id: u32,
    x: f32,
    y: f32,
    rotation: f32,
    #[serde(flatten)]
    layer: Layer,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    version: u32,
    background: String,
    stickers: BTreeMap<String, String>,
    frames: Pair<Frame>,
    covers: Pair<Vec<Element>>,
    sync_covers: bool,
}

pub fn jpeg_url(bytes: &[u8]) -> String {
    format!("data:image/jpeg;base64,{}", STANDARD.encode(bytes))
}

// Bound both the wire size and decoded surfaces before allocating pixels.
fn image(url: &str, side: i32) -> Result<Image> {
    let (format, data) = url.split_once(',').ok_or("缺少封面图片数据")?;
    let expected = match format {
        "data:image/png;base64" => EncodedImageFormat::PNG,
        "data:image/jpeg;base64" => EncodedImageFormat::JPEG,
        _ => return Err("不支持的封面图片格式".into()),
    };
    let bytes = STANDARD.decode(data).map_err(|_| "封面图片编码损坏")?;
    let mut codec = Codec::from_data(Data::new_copy(&bytes)).ok_or("封面图片损坏")?;
    let d = codec.dimensions();
    if codec.encoded_format() != expected || d.width.max(d.height) > side {
        return Err("封面图片格式或尺寸不正确".into());
    }
    codec
        .get_image(None, None)
        .map_err(|_| "无法解码封面图片".into())
}

fn color(hex: &str) -> Result<Color> {
    let rgb = hex
        .strip_prefix('#')
        .filter(|s| s.len() == 6)
        .and_then(|s| u32::from_str_radix(s, 16).ok())
        .ok_or("无效的封面颜色")?;
    Ok(Color::new(0xff000000 | rgb))
}

fn fonts() -> FontCollection {
    let mut fonts = FontCollection::new();
    fonts.set_default_font_manager(FontMgr::new(), None);
    fonts
}

impl Text {
    fn style(&self) -> Result<TextStyle> {
        let mut families = vec![
            "PingFang SC",
            "Microsoft YaHei",
            "Noto Sans CJK SC",
            "WenQuanYi Micro Hei",
            "sans-serif",
        ];
        if !self.font.is_empty() {
            families.insert(0, &self.font);
        }
        let mut style = TextStyle::new();
        style
            .set_font_families(&families)
            .set_font_size(self.font_size)
            .set_font_style(if self.bold {
                FontStyle::bold()
            } else {
                FontStyle::normal()
            })
            .set_color(color(&self.ink.fill)?);
        Ok(style)
    }
}

fn line(text: &str, style: &TextStyle, fonts: &FontCollection) -> Paragraph {
    let mut p = ParagraphStyle::new();
    p.set_text_style(style);
    let mut builder = ParagraphBuilder::new(&p, fonts.clone());
    builder.add_text(text);
    let mut p = builder.build();
    p.layout(1_000_000.);
    p
}

fn draw_text(canvas: &Canvas, text: &Text, fonts: &FontCollection) -> Result<()> {
    let size = text.font_size;
    let ink = &text.ink;
    if !(12.0..=800.).contains(&size)
        || !(0.0..=1.).contains(&ink.stroke_width)
        || text.content.len() > 8192
        || text.content.lines().count() > 128
        || text.font.len() > 256
        || text.font.contains('\0')
    {
        return Err("文字图层超出限制".into());
    }
    let mut style = text.style()?;
    let mut outline = style.clone();
    if let Some(stroke) = &ink.stroke {
        let mut pen = Paint::default();
        pen.set_anti_alias(true)
            .set_style(paint::Style::Stroke)
            .set_stroke_join(paint::Join::Round)
            .set_stroke_width(size * ink.stroke_width * 2.)
            .set_color(color(stroke)?);
        outline.set_foreground_paint(&pen);
    }
    if ink.shadow && ink.background.is_none() {
        let shadow_style = if ink.stroke.is_some() {
            &mut outline
        } else {
            &mut style
        };
        shadow_style.add_shadow(TextShadow::new(
            Color::from_argb(140, 0, 0, 0),
            (size * 0.03, size * 0.05),
            (size * 0.075) as f64,
        ));
    }
    let lines: Vec<_> = text
        .content
        .split('\n')
        .map(|s| (s, line(s, &style, fonts)))
        .collect();
    let width = lines
        .iter()
        .map(|(_, p)| p.longest_line())
        .fold(size * 0.5, f32::max);
    let height = lines.len() as f32 * size * 1.25;
    if let Some(bg) = &ink.background {
        let mut paint = Paint::default();
        paint.set_color(color(bg)?).set_anti_alias(true);
        let rect = Rect::from_xywh(
            -width / 2. - size * 0.35,
            -height / 2. - size * 0.12,
            width + size * 0.7,
            height + size * 0.24,
        );
        if ink.shadow {
            paint.set_image_filter(image_filters::drop_shadow(
                (size * 0.03, size * 0.05),
                (size * 0.075, size * 0.075),
                Color::from_argb(140, 0, 0, 0),
                None,
                None,
                None,
            ));
        }
        canvas.draw_round_rect(rect, size * 0.18, size * 0.18, &paint);
    }
    for (i, (s, p)) in lines.into_iter().enumerate() {
        let w = p.longest_line();
        let x = match text.align {
            Align::Left => -width / 2.,
            Align::Center => -w / 2.,
            Align::Right => width / 2. - w,
        };
        let y = -height / 2. + (i as f32 + 0.5) * size * 1.25 - p.height() / 2.;
        if ink.stroke.is_some() {
            line(s, &outline, fonts).paint(canvas, (x, y));
        }
        p.paint(canvas, (x, y));
    }
    Ok(())
}

fn draw_image(canvas: &Canvas, img: &Image, rect: Rect, paint: &Paint) {
    canvas.draw_image_rect_with_sampling_options(img, None, rect, FilterMode::Linear, paint);
}

fn render(
    width: i32,
    bg: &Image,
    stickers: &BTreeMap<String, Image>,
    fonts: &FontCollection,
    frame: &Frame,
    elements: &[Element],
) -> Result<Vec<u8>> {
    let w = width as f32;
    let sw = HEIGHT * bg.width() as f32 / bg.height() as f32;
    let min_zoom = sw.min(w) / sw.max(w);
    if !(min_zoom * 0.999..=4.001).contains(&frame.zoom)
        || !(0.0..=sw + 1.).contains(&frame.x)
        || !(0.0..=HEIGHT + 1.).contains(&frame.y)
    {
        return Err("无效的封面取景".into());
    }
    let k = w * frame.zoom / sw.min(w);
    let rect = Rect::from_xywh(
        w / 2. - frame.x * k,
        HEIGHT / 2. - frame.y * k,
        sw * k,
        HEIGHT * k,
    );
    let mut surface =
        surfaces::raster_n32_premul((width, HEIGHT as i32)).ok_or("无法分配封面画布")?;
    let canvas = surface.canvas();
    canvas.clear(Color::BLACK);
    let mut paint = Paint::default();
    if rect.left > 0.5 || rect.top > 0.5 || rect.right < w - 0.5 || rect.bottom < HEIGHT - 0.5 {
        let scale = (w / sw).max(1.);
        // Crop before filtering, so a panoramic source never needs a panoramic intermediate.
        let mut small = surfaces::raster_n32_premul((width / 32, 34)).ok_or("无法分配封面背景")?;
        let c = small.canvas();
        c.scale((1. / 32., 34. / HEIGHT));
        draw_image(
            c,
            bg,
            Rect::from_xywh(
                (w - sw * scale) / 2.,
                (HEIGHT - HEIGHT * scale) / 2.,
                sw * scale,
                HEIGHT * scale,
            ),
            &paint,
        );
        draw_image(
            canvas,
            &small.image_snapshot(),
            Rect::from_wh(w, HEIGHT),
            &paint,
        );
        paint.set_color(Color::from_argb(77, 0, 0, 0));
        canvas.draw_rect(Rect::from_wh(w, HEIGHT), &paint);
        paint = Paint::default();
    }
    draw_image(canvas, bg, rect, &paint);
    canvas.translate(((w - WIDE as f32) / 2., 0.));
    let mut ids = std::collections::HashSet::new();
    for e in elements {
        if !ids.insert(e.id)
            || !(0.0..=1920.).contains(&e.x)
            || !(0.0..=HEIGHT).contains(&e.y)
            || e.rotation.abs() > 360.
        {
            return Err("无效的封面图层位置或编号".into());
        }
        canvas.save();
        canvas.translate((e.x, e.y));
        canvas.rotate(e.rotation, None);
        match &e.layer {
            Layer::Text(t) => draw_text(canvas, t, fonts)?,
            Layer::Sticker {
                src,
                size,
                aspect,
                flip,
            } => {
                if !(1.0..=1600.).contains(size) || *aspect <= 0. {
                    return Err("无效的贴纸尺寸".into());
                }
                let img = stickers.get(src).ok_or("工程缺少贴纸素材")?;
                let (w, h) = (size * aspect.min(1.), size / aspect.max(1.));
                if *flip {
                    canvas.scale((-1., 1.));
                }
                draw_image(canvas, img, Rect::from_xywh(-w / 2., -h / 2., w, h), &paint);
            }
        }
        canvas.restore();
    }
    surface
        .image_snapshot()
        .encode(None, EncodedImageFormat::JPEG, 92)
        .map(|data| data.as_bytes().to_vec())
        .ok_or_else(|| "封面 JPEG 编码失败".into())
}

impl Project {
    pub fn render(&self) -> Result<Pair<Vec<u8>>> {
        if self.version != 1
            || self.stickers.len() > 64
            || self.covers.wide.len().max(self.covers.home.len()) > 256
            || self.background.len() + self.stickers.values().map(String::len).sum::<usize>()
                > 48 * 1024 * 1024
        {
            return Err("不支持的封面工程版本或工程过大".into());
        }
        let bg = image(&self.background, 3840)?;
        let mut stickers = BTreeMap::new();
        let mut pixels = 0;
        for (key, data) in &self.stickers {
            let img = image(data, 1024)?;
            pixels += img.width() * img.height();
            if pixels > 16 * 1024 * 1024 {
                return Err("贴纸素材过大".into());
            }
            stickers.insert(key.clone(), img);
        }
        let fonts = fonts();
        Ok(Pair {
            wide: render(
                WIDE,
                &bg,
                &stickers,
                &fonts,
                &self.frames.wide,
                &self.covers.wide,
            )?,
            home: render(
                HOME,
                &bg,
                &stickers,
                &fonts,
                &self.frames.home,
                &self.covers.home,
            )?,
        })
    }
}

/// Create ordinary editable layers, including explicit line breaks so reopening
/// in a browser need not repeat font selection or AI layout.
pub fn design(background: String, title: &str, style: &str, x: f32, y: f32) -> Result<Project> {
    let title = title.trim();
    if title.is_empty() || title.chars().count() > 256 {
        return Err("封面标题需要 1 至 256 个字符".into());
    }
    let bg = image(&background, 3840)?;
    let sw = HEIGHT * bg.width() as f32 / bg.height() as f32;
    let frame = |w: f32| {
        let kept = sw.min(w) / sw.max(w);
        Frame {
            x: sw / 2.,
            y: HEIGHT / 2.,
            zoom: if kept < 0.5 { kept } else { 1. },
        }
    };
    let mut presets: BTreeMap<String, Ink> =
        serde_json::from_str(include_str!("cover-styles.json")).map_err(|e| e.to_string())?;
    let ink = presets
        .remove(style)
        .or_else(|| presets.remove("yellow"))
        .ok_or("缺少封面样式")?;
    let mut text = Text {
        ink,
        content: String::new(),
        font: String::new(),
        font_size: 150.,
        bold: true,
        align: Align::Center,
    };
    let fonts = fonts();
    let (width, height) = loop {
        let style = text.style()?;
        let pad = if text.ink.background.is_some() {
            0.7
        } else {
            2. * text.ink.stroke_width + 0.1
        } * text.font_size;
        let mut rows = vec![String::new()];
        let mut width = 0f32;
        for g in title.graphemes(true) {
            let row = rows.last_mut().unwrap();
            if g == "\n" {
                rows.push(String::new());
                continue;
            }
            let next = format!("{row}{g}");
            let measured = line(&next, &style, &fonts).longest_line();
            if !row.is_empty() && measured + pad > 1360. {
                rows.push(g.into());
                width = width.max(line(g, &style, &fonts).longest_line());
            } else {
                *row = next;
                width = width.max(measured);
            }
        }
        let height = (1.25 * rows.len() as f32 + 0.25) * text.font_size;
        if (rows.len() <= 3 || text.font_size <= 40.) && height <= 1000. && width + pad <= 1360. {
            text.content = rows.join("\n");
            break (width + pad, height);
        }
        if text.font_size <= 40. {
            return Err("标题过长，无法完整放入封面".into());
        }
        text.font_size = (text.font_size - 8.).max(40.);
    };
    let element = Element {
        id: 1,
        x: (x * 1920.).clamp(264. + width / 2., 1656. - width / 2.),
        y: (y * HEIGHT).clamp(24. + height / 2., 1056. - height / 2.),
        rotation: 0.,
        layer: Layer::Text(text),
    };
    Ok(Project {
        version: 1,
        background,
        stickers: BTreeMap::new(),
        frames: Pair {
            wide: frame(1920.),
            home: frame(1440.),
        },
        covers: Pair {
            wide: vec![element.clone()],
            home: vec![element],
        },
        sync_covers: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(color: Color) -> String {
        let mut surface = surfaces::raster_n32_premul((16, 9)).unwrap();
        surface.canvas().clear(color);
        let png = surface
            .image_snapshot()
            .encode(None, EncodedImageFormat::PNG, None)
            .unwrap();
        format!("data:image/png;base64,{}", STANDARD.encode(png.as_bytes()))
    }

    #[test]
    fn title_is_editable_wrapped_and_inside_both_covers() {
        let title = "这是一个很长的标题，需要换行并且在两种封面中保持可见";
        let project = design(asset(Color::BLACK), title, "unknown", 0., 1.).unwrap();
        let e = &project.covers.home[0];
        let Layer::Text(t) = &e.layer else {
            panic!("expected editable title")
        };
        assert_eq!(t.content.replace('\n', ""), title);
        assert!((2..=3).contains(&t.content.lines().count()));
        assert_eq!(t.ink.fill, "#FFE14D");
        assert_eq!(e.id, project.covers.wide[0].id);
        let widest = t
            .content
            .lines()
            .map(|s| line(s, &t.style().unwrap(), &fonts()).longest_line())
            .fold(0., f32::max);
        assert!(e.x - widest / 2. >= 264.);
        assert!(e.y + (t.content.lines().count() as f32) * t.font_size * 0.625 <= 1056.);
    }

    #[test]
    fn independent_sticker_layers_survive_json_and_render_into_the_expected_jpeg() {
        let mut project = design(asset(Color::BLACK), "测试", "white", 0.5, 0.8).unwrap();
        project.stickers.insert("asset:0".into(), asset(Color::RED));
        project.covers.home.push(Element {
            id: 2,
            x: 960.,
            y: 540.,
            rotation: 30.,
            layer: Layer::Sticker {
                src: "asset:0".into(),
                size: 160.,
                aspect: 16. / 9.,
                flip: true,
            },
        });
        let loaded: Project =
            serde_json::from_slice(&serde_json::to_vec(&project).unwrap()).unwrap();
        let pair = loaded.render().unwrap();
        for (width, data, red) in [(WIDE, pair.wide, false), (HOME, pair.home, true)] {
            let mut codec = Codec::from_data(Data::new_copy(&data)).unwrap();
            assert_eq!(codec.encoded_format(), EncodedImageFormat::JPEG);
            assert_eq!(
                (codec.dimensions().width, codec.dimensions().height),
                (width, 1080)
            );
            let img = codec.get_image(None, None).unwrap();
            let p = img.peek_pixels().unwrap().get_color((width / 2, 540));
            assert_eq!(p.r() > 200, red);
            assert!(p.b() < 8 && p.g() < 8);
        }
        project.stickers.clear();
        assert!(project.render().is_err());
        project.version = 2;
        assert!(project.render().is_err());
    }
}
