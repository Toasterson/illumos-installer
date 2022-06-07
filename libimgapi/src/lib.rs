mod manifest;

pub use manifest::*;

#[cfg(test)]
mod tests {
    use crate::Manifest;

    static IMGAPI_PUBLIC_SERVER_LIST_URL: &str = "https://images.smartos.org/images";
    #[test]
    fn test_manifest_parsing() {
        let resp = reqwest::blocking::get(IMGAPI_PUBLIC_SERVER_LIST_URL).unwrap();
        let images: Vec<Manifest> = resp.json().unwrap();
        println!("NAME\tVERSION\tUUID\tIMAGE TYPE\tPUBLISHED AT");
        for image in images {
            let published_at = if let Some(published_at) = image.published_at {
                published_at.to_string()
            } else {
                "None".into()
            };
            println!(
                "{}\t{}\t{}\t{}\t{}",
                image.name, image.version, image.uuid, image.image_type, published_at
            );
        }
    }
}
