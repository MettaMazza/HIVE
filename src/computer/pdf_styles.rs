pub const BASE_CSS: &str = r#"
@import url('https://fonts.googleapis.com/css2?family=Inter:wght@400;600;800&family=Merriweather:ital,wght@0,300;0,700;1,300&family=Fira+Code&display=swap');

:root {
    --bg-color: #ffffff;
    --text-color: #1a1a1a;
    --heading-color: #000000;
    --accent-color: #2563eb;
    --border-color: #e5e7eb;
    --code-bg: #f3f4f6;
    --font-sans: 'Inter', system-ui, sans-serif;
    --font-serif: 'Merriweather', Georgia, serif;
    --font-mono: 'Fira Code', monospace;
}

html {
    width: 100% !important;
    height: 100% !important;
    margin: 0 !important;
    padding: 0 !important;
    -webkit-print-color-adjust: exact !important;
    print-color-adjust: exact !important;
}

body {
    font-family: var(--font-sans);
    line-height: 1.6;
    color: var(--text-color);
    background: var(--bg-color) !important;
    margin: 0 !important;
    padding: 0 !important;
    width: 100vw !important;
    height: 100vh !important;
    max-width: 100% !important;
    -webkit-print-color-adjust: exact !important;
    print-color-adjust: exact !important;
    box-sizing: border-box !important;
}

/* Print Setup for A4 */
@page {
    size: A4;
    margin: 0;
}

.document-container {
    padding: 2cm;
    box-sizing: border-box !important;
}

.header {
    text-align: center;
    border-bottom: 2px solid var(--accent-color);
    margin-bottom: 2rem;
    padding-bottom: 1rem;
}

.header h1 {
    font-size: 2.5rem;
    color: var(--heading-color);
    margin: 0 0 0.5rem 0;
    font-weight: 800;
    letter-spacing: -0.02em;
}

.header .author {
    font-size: 1.1rem;
    color: var(--accent-color);
    font-weight: 600;
}

.section {
    margin-top: 2rem;
    page-break-inside: avoid;
}

.section h2 {
    color: var(--heading-color);
    font-size: 1.8rem;
    border-bottom: 1px solid var(--border-color);
    padding-bottom: 0.3rem;
    margin-bottom: 1rem;
}

h1, h2, h3, h4, h5, h6 {
    line-height: 1.2;
    margin-top: 1.5em;
}

p {
    margin-bottom: 1em;
}

pre {
    background: var(--code-bg);
    padding: 1rem;
    border-radius: 0.5rem;
    overflow-x: auto;
    font-family: var(--font-mono);
    font-size: 0.9em;
    border: 1px solid var(--border-color);
}

code {
    font-family: var(--font-mono);
    background: var(--code-bg);
    padding: 0.2em 0.4em;
    border-radius: 3px;
    font-size: 0.9em;
}

blockquote {
    border-left: 4px solid var(--accent-color);
    margin: 1.5em 0;
    padding: 0.5em 0 0.5em 1.5em;
    font-style: italic;
    color: var(--text-color);
    opacity: 0.8;
}

table {
    width: 100%;
    border-collapse: collapse;
    margin: 1.5em 0;
}

th, td {
    padding: 0.75rem;
    border: 1px solid var(--border-color);
    text-align: left;
}

th {
    background: var(--code-bg);
    font-weight: 600;
}

img {
    max-width: 100%;
    height: auto;
    border-radius: 0.5rem;
    box-shadow: 0 8px 16px rgba(0,0,0,0.2);
    margin: 1.5rem auto;
    display: block;
    border: 1px solid rgba(255, 255, 255, 0.1);
}
"#;

pub fn get_theme(name: &str) -> &'static str {
    match name {
        "academic" => {
            r#"
            :root {
                --font-sans: 'Merriweather', Georgia, serif;
                --heading-color: #111827;
                --accent-color: #4b5563;
                --border-color: #d1d5db;
                --bg-color: #fdfbf7;
            }
            .header { text-align: left; border-bottom: 3px double var(--accent-color); }
            p { text-align: justify; }
        "#
        }
        "dark" => {
            r#"
            :root {
                --bg-color: #111827;
                --text-color: #e5e7eb;
                --heading-color: #f9fafb;
                --accent-color: #60a5fa;
                --border-color: #374151;
                --code-bg: #1f2937;
            }
        "#
        }
        "cyberpunk" => {
            r#"
            @import url('https://fonts.googleapis.com/css2?family=Share+Tech+Mono&display=swap');
            :root {
                --bg-color: #050505;
                --text-color: #00ff41;
                --heading-color: #ff003c;
                --accent-color: #00f0ff;
                --border-color: #333333;
                --code-bg: #111111;
                --font-sans: 'Share Tech Mono', monospace;
                --font-serif: 'Share Tech Mono', monospace;
            }
            body { text-transform: uppercase; }
            .header, .section h2 { border-color: var(--accent-color); text-shadow: 0 0 5px var(--accent-color); }
        "#
        }
        "pastel" => {
            r#"
            :root {
                --bg-color: #faf5ff;
                --text-color: #4c1d95;
                --heading-color: #5b21b6;
                --accent-color: #d8b4fe;
                --border-color: #e9d5ff;
                --code-bg: #f3e8ff;
                --font-sans: 'Inter', system-ui, sans-serif;
            }
            .header h1 { color: #db2777; }
        "#
        }
        "minimal" => {
            r#"
            :root {
                --accent-color: #000000;
                --border-color: #e0e0e0;
            }
            .header { text-align: left; border-bottom: none; }
            .header h1 { font-size: 2rem; font-weight: 500; }
            .section h2 { border-bottom: none; font-weight: 600; text-transform: uppercase; font-size: 1.2rem; letter-spacing: 0.05em; margin-top: 3rem; }
            p { color: #444; }
        "#
        }
        "elegant" => {
            r#"
            @import url('https://fonts.googleapis.com/css2?family=Cormorant+Garamond:ital,wght@0,400;0,600;1,400&family=Montserrat:wght@300;400;600&display=swap');
            :root {
                --bg-color: #fdfdfc;
                --text-color: #2c3e50;
                --heading-color: #1a252f;
                --accent-color: #c0392b;
                --border-color: #bdc3c7;
                --font-sans: 'Montserrat', sans-serif;
                --font-serif: 'Cormorant Garamond', serif;
            }
            body { font-family: var(--font-serif); font-size: 1.1em; }
            h1, h2, h3, .author { font-family: var(--font-sans); }
            p { font-weight: 400; }
            .header h1 { font-weight: 300; letter-spacing: 0.1em; text-transform: uppercase; }
        "#
        }
        // Default professional theme
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_theme() {
        assert!(get_theme("academic").contains("Merriweather"));
        assert!(get_theme("dark").contains("#111827"));
        assert!(get_theme("cyberpunk").contains("Share Tech Mono"));
        assert!(get_theme("pastel").contains("#faf5ff"));
        assert!(get_theme("minimal").contains("border-bottom: none"));
        assert!(get_theme("elegant").contains("Cormorant Garamond"));
        assert_eq!(get_theme("unknown_default_theme"), "");
    }
}
