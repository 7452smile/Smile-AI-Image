use std::path::Path;

use image::ImageReader;

use crate::{GenerationParameters, ImageModel, PixelDimensions, Resolution, is_gpt_image_model};

// Google image-generation size table: https://ai.google.dev/gemini-api/docs/image-generation
pub(crate) fn banana_dimensions(
    model: ImageModel,
    resolution: Resolution,
    ratio: &str,
) -> Option<PixelDimensions> {
    if !matches!(model, ImageModel::NanoBanana2 | ImageModel::NanoBanana21) {
        return None;
    }
    let (width, height) = match ratio {
        "1:1" => (1024, 1024),
        "1:4" => (512, 2048),
        "1:8" => (384, 3072),
        "2:3" => (848, 1264),
        "3:2" => (1264, 848),
        "3:4" => (896, 1200),
        "4:1" => (2048, 512),
        "4:3" => (1200, 896),
        "4:5" => (928, 1152),
        "5:4" => (1152, 928),
        "8:1" => (3072, 384),
        "9:16" => (768, 1376),
        "16:9" => (1376, 768),
        "21:9" => (1584, 672),
        _ => return None,
    };
    let scale = match resolution {
        Resolution::Auto | Resolution::OneK => 1,
        Resolution::TwoK => 2,
        Resolution::FourK => 4,
    };
    Some(PixelDimensions {
        width: width * scale,
        height: height * scale,
    })
}

pub(crate) fn inspect(
    path: &Path,
    parameters: &GenerationParameters,
) -> (Option<PixelDimensions>, Vec<String>) {
    // Read headers only. Do not decode or resize high-resolution pixels.
    let dimensions = ImageReader::open(path)
        .and_then(|reader| reader.with_guessed_format())
        .ok()
        .and_then(|reader| reader.into_dimensions().ok())
        .map(|(width, height)| PixelDimensions { width, height });
    let warnings = dimension_warnings(dimensions, parameters);
    (dimensions, warnings)
}

fn dimension_warnings(
    actual: Option<PixelDimensions>,
    parameters: &GenerationParameters,
) -> Vec<String> {
    let Some(actual) = actual else {
        return vec![
            "无法读取原图像素尺寸，分辨率尚未验证；请检查已保存的原图。不要自动重新生成。".into(),
        ];
    };
    if let Some(expected) = parameters.expected_dimensions {
        if actual != expected {
            return vec![format!(
                "请求模型 {}，尺寸 {}（期望 {}×{}），但上游原图实际为 {}×{}。图片已保存；请报告尺寸不符，不要声称已达到请求分辨率，也不要自动重新生成。",
                parameters.api_model,
                parameters.image_size,
                expected.width,
                expected.height,
                actual.width,
                actual.height,
            )];
        }
    } else if !is_gpt_image_model(parameters.model) {
        // With automatic aspect ratio, compare area; long edges can be large even at 1K.
        // Allow 10% rounding differences across ratios and upstream model revisions.
        let nominal_area = match parameters.resolution {
            Resolution::Auto | Resolution::OneK => 1024_u64 * 1024,
            Resolution::TwoK => 2048_u64 * 2048,
            Resolution::FourK => 4096_u64 * 4096,
        };
        if u64::from(actual.width) * u64::from(actual.height) * 10 < nominal_area * 9 {
            return vec![format!(
                "请求模型 {}，尺寸 {}，但上游原图实际为 {}×{}，像素数量低于请求档位。图片已保存；请报告分辨率不足，不要自动重新生成。",
                parameters.api_model, parameters.image_size, actual.width, actual.height,
            )];
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use image::{DynamicImage, ImageFormat};

    use super::*;

    #[test]
    fn reads_saved_png_jpeg_and_webp_dimensions() {
        let dir = tempfile::tempdir().unwrap();
        let parameters = crate::generation_parameters(
            ImageModel::GptImage2,
            Resolution::Auto,
            "auto",
            crate::ImageQuality::Auto,
        )
        .unwrap();
        for (extension, format) in [
            ("png", ImageFormat::Png),
            ("jpg", ImageFormat::Jpeg),
            ("webp", ImageFormat::WebP),
        ] {
            let path = dir.path().join(format!("image.{extension}"));
            DynamicImage::new_rgb8(11, 7)
                .save_with_format(&path, format)
                .unwrap();
            let (dimensions, warnings) = inspect(&path, &parameters);
            assert_eq!(
                dimensions,
                Some(PixelDimensions {
                    width: 11,
                    height: 7
                })
            );
            assert!(warnings.is_empty());
        }
    }

    #[test]
    fn flags_1k_returned_for_4k_including_auto_ratio_and_pro() {
        for model in [
            ImageModel::NanoBanana2,
            ImageModel::NanoBanana21,
            ImageModel::NanoBananaPro,
        ] {
            for ratio in ["auto", "1:1"] {
                let parameters = crate::generation_parameters(
                    model,
                    Resolution::FourK,
                    ratio,
                    crate::ImageQuality::Auto,
                )
                .unwrap();
                let warnings = dimension_warnings(
                    Some(PixelDimensions {
                        width: 1024,
                        height: 1024,
                    }),
                    &parameters,
                );
                assert_eq!(warnings.len(), 1);
                assert!(warnings[0].contains("1024×1024"));
                assert!(warnings[0].contains("不要自动重新生成"));
                assert!(
                    dimension_warnings(
                        Some(PixelDimensions {
                            width: 4096,
                            height: 4096
                        }),
                        &parameters
                    )
                    .is_empty()
                );
            }
        }
    }

    #[test]
    fn extended_1k_ratio_cannot_pass_as_4k() {
        let parameters = crate::generation_parameters(
            ImageModel::NanoBanana21,
            Resolution::FourK,
            "auto",
            crate::ImageQuality::Auto,
        )
        .unwrap();
        assert!(
            !dimension_warnings(
                Some(PixelDimensions {
                    width: 3072,
                    height: 384
                }),
                &parameters
            )
            .is_empty()
        );
        assert!(
            dimension_warnings(
                Some(PixelDimensions {
                    width: 12288,
                    height: 1536
                }),
                &parameters
            )
            .is_empty()
        );
    }

    #[test]
    fn reports_unreadable_dimensions_without_discarding_image() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("invalid.png");
        std::fs::write(&path, b"invalid image").unwrap();
        let parameters = crate::generation_parameters(
            ImageModel::NanoBanana21,
            Resolution::FourK,
            "auto",
            crate::ImageQuality::Auto,
        )
        .unwrap();
        let (dimensions, warnings) = inspect(&path, &parameters);
        assert!(dimensions.is_none());
        assert_eq!(warnings.len(), 1);
        assert!(path.exists());
    }
}
