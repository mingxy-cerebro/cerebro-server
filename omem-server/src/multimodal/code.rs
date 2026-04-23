use crate::domain::error::OmemError;

use super::service::ProcessedChunk;

pub struct CodeProcessor;

impl CodeProcessor {
    pub fn extract(
        data: &[u8],
        language: &str,
        filename: &str,
    ) -> Result<Vec<ProcessedChunk>, OmemError> {
        let source = String::from_utf8_lossy(data);
        if source.trim().is_empty() {
            return Ok(Vec::new());
        }

        match Self::ast_chunk(&source, language, filename) {
            Ok(chunks) if !chunks.is_empty() => Ok(chunks),
            _ => Self::naive_chunk(&source, language, filename),
        }
    }

    fn ast_chunk(
        source: &str,
        language: &str,
        filename: &str,
    ) -> Result<Vec<ProcessedChunk>, OmemError> {
        let ts_language = Self::get_tree_sitter_language(language)
            .ok_or_else(|| OmemError::Internal(format!("unsupported language: {language}")))?;

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&ts_language)
            .map_err(|e| OmemError::Internal(format!("failed to set language: {e}")))?;

        let tree = parser
            .parse(source, None)
            .ok_or_else(|| OmemError::Internal("tree-sitter parse returned None".to_string()))?;

        let root = tree.root_node();
        if root.has_error() {
            return Err(OmemError::Internal("parse tree has errors".to_string()));
        }

        let mut chunks = Vec::new();
        Self::collect_function_nodes(root, source, language, filename, &mut chunks);

        Ok(chunks)
    }

    fn collect_function_nodes(
        node: tree_sitter::Node,
        source: &str,
        language: &str,
        filename: &str,
        chunks: &mut Vec<ProcessedChunk>,
    ) {
        let kind = node.kind();
        let is_function = matches!(
            kind,
            "function_item"
                | "impl_item"
                | "function_definition"
                | "class_definition"
                | "function_declaration"
                | "method_definition"
                | "class_declaration"
                | "arrow_function"
                | "method_declaration"
                | "interface_declaration"
                | "type_alias_declaration"
        );

        if is_function {
            let start_line = node.start_position().row + 1;
            let end_line = node.end_position().row + 1;
            let text = &source[node.byte_range()];

            let name = Self::extract_name(node, source).unwrap_or_else(|| kind.to_string());

            chunks.push(ProcessedChunk {
                content: text.to_string(),
                chunk_type: "code_function".to_string(),
                metadata: serde_json::json!({
                    "filename": filename,
                    "language": language,
                    "name": name,
                    "kind": kind,
                    "start_line": start_line,
                    "end_line": end_line,
                }),
            });
            return;
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            Self::collect_function_nodes(child, source, language, filename, chunks);
        }
    }

    fn extract_name(node: tree_sitter::Node, source: &str) -> Option<String> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            let kind = child.kind();
            if kind == "identifier"
                || kind == "name"
                || kind == "type_identifier"
                || kind == "property_identifier"
            {
                return Some(source[child.byte_range()].to_string());
            }
        }
        None
    }

    fn get_tree_sitter_language(language: &str) -> Option<tree_sitter::Language> {
        match language {
            "rust" => Some(tree_sitter_rust::LANGUAGE.into()),
            "python" => Some(tree_sitter_python::LANGUAGE.into()),
            "javascript" | "jsx" => Some(tree_sitter_javascript::LANGUAGE.into()),
            "typescript" | "tsx" => Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
            _ => None,
        }
    }

    fn naive_chunk(
        source: &str,
        language: &str,
        filename: &str,
    ) -> Result<Vec<ProcessedChunk>, OmemError> {
        let mut chunks = Vec::new();
        let mut current_chunk = String::new();
        let mut chunk_start_line = 1usize;
        let mut line_num = 0usize;

        for line in source.lines() {
            line_num += 1;
            if line.trim().is_empty() && !current_chunk.trim().is_empty() {
                let trimmed = current_chunk.trim().to_string();
                if !trimmed.is_empty() {
                    chunks.push(ProcessedChunk {
                        content: trimmed,
                        chunk_type: "code_block".to_string(),
                        metadata: serde_json::json!({
                            "filename": filename,
                            "language": language,
                            "start_line": chunk_start_line,
                            "end_line": line_num - 1,
                            "chunking": "naive",
                        }),
                    });
                }
                current_chunk.clear();
                chunk_start_line = line_num + 1;
            } else {
                if current_chunk.is_empty() {
                    chunk_start_line = line_num;
                }
                current_chunk.push_str(line);
                current_chunk.push('\n');
            }
        }

        if !current_chunk.trim().is_empty() {
            chunks.push(ProcessedChunk {
                content: current_chunk.trim().to_string(),
                chunk_type: "code_block".to_string(),
                metadata: serde_json::json!({
                    "filename": filename,
                    "language": language,
                    "start_line": chunk_start_line,
                    "end_line": line_num,
                    "chunking": "naive",
                }),
            });
        }

        Ok(chunks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_python_ast_chunking() {
        let source = r#"
def hello():
    print("hello")

def world():
    print("world")

class MyClass:
    def method(self):
        pass
"#;
        let chunks = CodeProcessor::extract(source.as_bytes(), "python", "test.py").unwrap();
        assert!(
            !chunks.is_empty(),
            "should extract at least one function chunk"
        );

        let names: Vec<&str> = chunks
            .iter()
            .filter_map(|c| c.metadata["name"].as_str())
            .collect();
        assert!(names.contains(&"hello"), "should find 'hello' function");
        assert!(names.contains(&"world"), "should find 'world' function");
    }

    #[test]
    fn test_rust_ast_chunking() {
        let source = r#"
fn add(a: i32, b: i32) -> i32 {
    a + b
}

fn subtract(a: i32, b: i32) -> i32 {
    a - b
}
"#;
        let chunks = CodeProcessor::extract(source.as_bytes(), "rust", "lib.rs").unwrap();
        assert!(chunks.len() >= 2, "should extract at least 2 functions");

        let names: Vec<&str> = chunks
            .iter()
            .filter_map(|c| c.metadata["name"].as_str())
            .collect();
        assert!(names.contains(&"add"));
        assert!(names.contains(&"subtract"));
    }

    #[test]
    fn test_javascript_ast_chunking() {
        let source = r#"
function greet(name) {
    return "Hello, " + name;
}

function farewell(name) {
    return "Goodbye, " + name;
}
"#;
        let chunks = CodeProcessor::extract(source.as_bytes(), "javascript", "app.js").unwrap();
        assert!(!chunks.is_empty());
    }

    #[test]
    fn test_naive_fallback_for_unknown_language() {
        let source = "block1 line1\nblock1 line2\n\nblock2 line1\nblock2 line2";
        let chunks = CodeProcessor::extract(source.as_bytes(), "unknown_lang", "file.xyz").unwrap();
        assert_eq!(chunks.len(), 2, "should split into 2 blocks by empty line");
        assert_eq!(chunks[0].chunk_type, "code_block");
        assert_eq!(chunks[0].metadata["chunking"], "naive");
    }

    #[test]
    fn test_code_chunk_metadata() {
        let source = "fn main() {\n    println!(\"hi\");\n}\n";
        let chunks = CodeProcessor::extract(source.as_bytes(), "rust", "main.rs").unwrap();
        assert!(!chunks.is_empty());
        let first = &chunks[0];
        assert_eq!(first.metadata["language"], "rust");
        assert_eq!(first.metadata["filename"], "main.rs");
        assert!(first.metadata["start_line"].as_u64().is_some());
        assert!(first.metadata["end_line"].as_u64().is_some());
    }

    #[test]
    fn test_empty_source() {
        let chunks = CodeProcessor::extract(b"", "rust", "empty.rs").unwrap();
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_invalid_syntax_falls_back_to_naive() {
        let source = "fn {{{ invalid rust syntax\n\nsome other block";
        let chunks = CodeProcessor::extract(source.as_bytes(), "rust", "bad.rs").unwrap();
        assert!(!chunks.is_empty());
        assert!(
            chunks.iter().any(|c| c.chunk_type == "code_block"),
            "should fall back to naive chunking"
        );
    }
}
