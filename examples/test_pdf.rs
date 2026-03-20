use headless_chrome::{Browser, LaunchOptions};
use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let html = r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <style>
        /* CRITICAL: A4 true dimensions with margin forced to 0 */
        @page { size: A4; margin: 0; }
        
        /* Force body to print background and fill space */
        html, body {
            width: 100%;
            height: 100%;
            margin: 0;
            padding: 0;
            background: #0f172a;
            color: #ffffff;
            -webkit-print-color-adjust: exact;
            print-color-adjust: exact;
        }

        .document-container {
            padding: 2cm;
            box-sizing: border-box; /* Ensure padding doesn't widen the div beyond 100% */
        }
    </style>
</head>
<body>
    <div class="document-container">
        <h1>Test PDF Boundaries</h1>
        <p>This background should be dark slate edge-to-edge.</p>
    </div>
</body>
</html>"#;

    let path = std::env::current_dir()?.join("test_bleed.html");
    fs::write(&path, html)?;

    let browser = Browser::new(LaunchOptions::default_builder().build()?)?;
    let tab = browser.new_tab()?;
    
    let file_url = format!("file://{}", path.canonicalize()?.to_string_lossy());
    tab.navigate_to(&file_url)?;
    tab.wait_until_navigated()?;

    let mut pdf_options = headless_chrome::types::PrintToPdfOptions::default();
    pdf_options.print_background = Some(true); // MUST be true
    pdf_options.prefer_css_page_size = Some(true); // Use @page
    
    // Explicitly zero the Chromium margins
    pdf_options.margin_top = Some(0.0);
    pdf_options.margin_bottom = Some(0.0);
    pdf_options.margin_left = Some(0.0);
    pdf_options.margin_right = Some(0.0);
    
    let pdf_data = tab.print_to_pdf(Some(pdf_options))?;
    fs::write("test_bleed.pdf", pdf_data)?;

    Ok(())
}
