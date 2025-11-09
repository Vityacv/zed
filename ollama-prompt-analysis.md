# Ollama Prompt Analysis and Improvements

## Current Implementation Analysis

### Ollama's Prompt Structure

The Ollama implementation uses a simple chat-based approach:

1. **System Message**: 
   - Defines the assistant's role as a code completion tool
   - Specifies markers for editable region and cursor position
   - Instructs to respond only with rewritten contents

2. **User Message Structure**:
   - Workspace summary (file, language, tab settings)
   - Recent buffer events (currently empty)
   - Diagnostics (currently empty)
   - Referenced declarations (currently empty)
   - Signatures (currently empty)
   - Editable excerpt with cursor marker

### Comparison with Other Providers

#### Copilot
- **Approach**: Direct API integration with GitHub Copilot service
- **Context**: Relies on Copilot's internal context understanding
- **Format**: Uses position-based ranges, not prompt-based
- **Strength**: Sophisticated context awareness built into the service

#### Supermaven
- **Approach**: Binary agent with streaming completions
- **Context**: Handles context internally
- **Format**: Returns completion text that's diffed against buffer
- **Strength**: Efficient diff-based completion matching

#### Zeta (Zed's Cloud Provider)
- **Approach**: Sophisticated prompt planning with budget management
- **Format**: Two prompt formats:
  1. **MarkedExcerpt**: Similar to Ollama but more refined
  2. **LabeledSections**: File-based sections with explicit labels
- **Context Features**:
  - Parent signature inclusion
  - Referenced declarations with scoring
  - Snippet prioritization based on relevance
  - Budget-aware context selection (10KB default)

## Issues with Current Ollama Implementation

### 1. **Ineffective Prompt Design**
- **Problem**: The prompt asks for "rewritten contents between the markers" but includes the entire excerpt
- **Issue**: This causes the model to regenerate everything instead of just completing at cursor
- **Impact**: Wasteful token usage and potentially confusing outputs

### 2. **Limited Context Window**
- Fixed 4KB prefix / 1KB suffix is too restrictive
- No dynamic adjustment based on model capabilities
- Missing actual context (diagnostics, references, signatures)

### 3. **Poor Response Processing**
- Expects the entire excerpt to be returned
- Has to trim redundant prefix/suffix after generation
- Inefficient use of model's generation capacity

### 4. **Missing Context Elements**
- No collection of referenced declarations
- No diagnostic information from LSP
- No signature help or type information
- No recent buffer events tracking

## Recommended Prompt Improvements

### Option 1: Fill-in-the-Middle (FIM) Approach
Many models support FIM format which is more efficient:

```rust
fn build_fim_prompt(context: &PromptContext) -> String {
    // Use model-specific FIM markers
    format!(
        "<|fim_prefix|>{}<|fim_suffix|>{}<|fim_middle|>",
        context.prefix,
        context.suffix
    )
}
```

### Option 2: Improved Chat-Based Prompt

```rust
const IMPROVED_SYSTEM_PROMPT: &str = r#"You are a code completion assistant.
Generate ONLY the code to insert at the cursor position.
Do not repeat existing code. Focus on completing the current statement or block.
Consider the language syntax and indentation style."#;

fn build_improved_messages(context: &PromptContext) -> Vec<ChatMessage> {
    let system = ChatMessage::System {
        content: IMPROVED_SYSTEM_PROMPT.into(),
    };
    
    let mut content = String::new();
    
    // Provide context without asking for full rewrite
    writeln!(&mut content, "Language: {}", context.language);
    writeln!(&mut content, "File: {}", context.file_path);
    writeln!(&mut content, "Indentation: {} spaces", context.tab_size);
    
    // Show limited context before cursor
    writeln!(&mut content, "\nContext before cursor (last 20 lines):");
    writeln!(&mut content, "```{}", context.language);
    writeln!(&mut content, "{}", last_n_lines(&context.prefix, 20));
    writeln!(&mut content, "```");
    
    // Show cursor position clearly
    writeln!(&mut content, "\nComplete the code at this position:");
    writeln!(&mut content, "```{}", context.language);
    writeln!(&mut content, "{}█", last_line_before_cursor(&context.prefix));
    writeln!(&mut content, "```");
    
    // Show context after cursor if relevant
    if !context.suffix.trim().is_empty() {
        writeln!(&mut content, "\nContext after cursor:");
        writeln!(&mut content, "```{}", context.language);
        writeln!(&mut content, "{}", first_n_lines(&context.suffix, 5));
        writeln!(&mut content, "```");
    }
    
    let user = ChatMessage::User {
        content,
        images: None,
    };
    
    vec![system, user]
}
```

### Option 3: Model-Specific Optimization

Different models work better with different prompt formats:

```rust
fn build_model_specific_prompt(model: &str, context: &PromptContext) -> Vec<ChatMessage> {
    match model {
        name if name.starts_with("codellama") => {
            // CodeLlama works well with FIM format
            build_fim_prompt(context)
        }
        name if name.contains("deepseek") => {
            // DeepSeek prefers structured prompts
            build_structured_prompt(context)
        }
        name if name.contains("qwen") && name.contains("coder") => {
            // Qwen-Coder has specific formatting
            build_qwen_prompt(context)
        }
        _ => {
            // Default to improved chat format
            build_improved_messages(context)
        }
    }
}
```

## Implementation Recommendations

### Immediate Fixes

1. **Change the prompt to request completion only**:
   - Don't ask for the entire excerpt to be rewritten
   - Focus on generating what comes after the cursor
   - Use clearer instructions about what to generate

2. **Reduce token waste**:
   - Show only relevant context (last N lines before cursor)
   - Don't include the full suffix unless necessary
   - Adjust context based on model's context window

3. **Better response processing**:
   - Expect only the completion text, not the full rewrite
   - Remove the complex prefix/suffix trimming logic
   - Handle incomplete lines better

### Advanced Improvements

1. **Implement context collection**:
   ```rust
   fn collect_diagnostics(buffer: &Buffer, cursor: Anchor) -> Vec<Diagnostic>
   fn collect_references(buffer: &Buffer, cursor: Anchor) -> Vec<Reference>
   fn collect_signatures(buffer: &Buffer, cursor: Anchor) -> Vec<Signature>
   ```

2. **Add model detection**:
   ```rust
   fn detect_model_capabilities(model: &str) -> ModelCapabilities {
       ModelCapabilities {
           supports_fim: model.contains("codellama") || model.contains("deepseek"),
           context_window: get_context_window(model),
           optimal_format: get_optimal_format(model),
       }
   }
   ```

3. **Implement streaming with partial acceptance**:
   - Stream tokens and update prediction incrementally
   - Allow accepting partial completions word by word
   - Cancel generation if user continues typing

## Example Improved Implementation

```rust
fn build_messages(context: &PromptContext, model: &str) -> Vec<ChatMessage> {
    // Detect if model supports FIM
    if supports_fim(model) {
        return vec![ChatMessage::User {
            content: format!(
                "<PRE>{}</PRE><SUF>{}</SUF><MID>",
                context.prefix.chars().rev().take(2000).collect::<String>().chars().rev().collect::<String>(),
                context.suffix.chars().take(500).collect::<String>()
            ),
            images: None,
        }];
    }
    
    // Otherwise use improved chat format
    let system = ChatMessage::System {
        content: "You are a code autocompletion engine. Respond with ONLY the code to insert at the cursor. No explanations.".into(),
    };
    
    let last_lines: Vec<&str> = context.prefix.lines().rev().take(10).collect::<Vec<_>>().into_iter().rev().collect();
    let content = format!(
        "Complete this {} code:\n\n{}\n[CURSOR]\n\nGenerate only what comes after [CURSOR].",
        context.language_name,
        last_lines.join("\n")
    );
    
    vec![
        system,
        ChatMessage::User {
            content,
            images: None,
        }
    ]
}
```

## Conclusion

The current Ollama prompt implementation is functional but inefficient. The main issues are:

1. **Asking for too much**: Requesting the entire excerpt be rewritten instead of just completion
2. **Wasting tokens**: Including unnecessary context and expecting verbose responses  
3. **Missing optimization**: Not using FIM format for models that support it
4. **Lacking context**: Not collecting valuable LSP information

With the recommended changes, the Ollama provider could be much more efficient and provide better completions while using fewer tokens.