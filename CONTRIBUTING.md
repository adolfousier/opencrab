# Contributing to OpenCrab

Thank you for your interest in contributing to OpenCrab! We welcome contributions from the community and are excited to work with you.

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [How Can I Contribute?](#how-can-i-contribute)
- [Development Setup](#development-setup)
- [Pull Request Process](#pull-request-process)
- [Coding Standards](#coding-standards)
- [Testing Guidelines](#testing-guidelines)
- [Documentation](#documentation)
- [License](#license)

## Code of Conduct

This project and everyone participating in it is governed by our commitment to creating a welcoming and inclusive environment. By participating, you are expected to:

- Use welcoming and inclusive language
- Be respectful of differing viewpoints and experiences
- Gracefully accept constructive criticism
- Focus on what is best for the community
- Show empathy towards other community members

## How Can I Contribute?

### Reporting Bugs

Before creating bug reports, please check existing issues to avoid duplicates. When creating a bug report, include:

- **Clear descriptive title** - A good title helps others find your issue
- **Detailed steps to reproduce** - Include specific examples to demonstrate the steps
- **Expected vs. actual behavior** - What you expected to happen and what actually happened
- **Environment details**:
  - OS and version (Windows/macOS/Linux)
  - Rust version (`rustc --version`)
  - OpenCrab version
  - Configuration details (redact any API keys)
- **Error messages and logs** - Include complete error messages and stack traces

### Suggesting Enhancements

Enhancement suggestions are tracked as GitHub issues. When creating an enhancement suggestion:

- **Use a clear and descriptive title**
- **Provide a detailed description** of the suggested enhancement
- **Explain why this enhancement would be useful** to most OpenCrab users
- **List any alternatives** you've considered
- **Include mockups or examples** if applicable

### Pull Requests

We actively welcome your pull requests! Here's how to get started:

1. Fork the repository and create your branch from `master`
2. Make your changes following our coding standards
3. Add or update tests as needed
4. Ensure the test suite passes
5. Update documentation to reflect your changes
6. Submit your pull request

## Development Setup

### Prerequisites

- **Rust** 1.70 or later
- **SQLite** (usually included with your system)
- **Git**

### Initial Setup

```bash
# Clone your fork
git clone https://github.com/adolfousier/opencrab.git
cd opencrab

# Build the project
cargo build

# Run tests
cargo test

# Run the application
cargo run
```

### Development Tools

We recommend installing these tools for a better development experience:

```bash
# Format checking
cargo install rustfmt

# Linting
cargo install clippy

# Code coverage
cargo install cargo-tarpaulin

# Auto-formatting on save (optional)
cargo install cargo-watch
```

### Running the Development Version

```bash
# Run with debug logging
RUST_LOG=debug cargo run

# Run specific command
cargo run -- init
cargo run -- db stats

# Run with local LLM (LM Studio)
export OPENAI_BASE_URL="http://localhost:1234/v1"
cargo run
```

## Pull Request Process

1. **Update Documentation**
   - Update README.md if you change functionality
   - Add or update code comments for complex logic
   - Update relevant guides in `docs/guides/`

2. **Add Tests**
   - Unit tests for new functionality
   - Integration tests for complex features
   - Ensure all tests pass: `cargo test`

3. **Check Code Quality**
   ```bash
   # Format your code
   cargo fmt

   # Run clippy (no warnings allowed)
   cargo clippy --all-targets --all-features -- -D warnings

   # Run all tests
   cargo test --all-features
   ```

4. **Update Changelog**
   - Add a brief description of your changes
   - Reference the issue number if applicable

5. **Create Pull Request**
   - Use a clear, descriptive title
   - Reference any related issues
   - Describe what your PR does and why
   - List any breaking changes
   - Include screenshots for UI changes

6. **Review Process**
   - Address any feedback from reviewers
   - Keep your PR up to date with master
   - Squash commits if requested

## Coding Standards

### Rust Style Guide

We follow the [Rust Style Guide](https://doc.rust-lang.org/style-guide/) with these additions:

- **Line length**: 100 characters (enforced by rustfmt)
- **Comments**: Use `///` for doc comments, `//` for inline comments
- **Error handling**: Use `anyhow::Result` for application errors, specific error types for libraries
- **Async code**: Use `tokio` runtime, avoid blocking calls in async functions

### Code Organization

```
src/
├── cli/          # Command-line interface
├── config/       # Configuration management
├── db/           # Database layer
├── llm/          # LLM provider integrations
│   ├── agent/    # Agent orchestration
│   ├── provider/ # Provider implementations
│   └── tools/    # Tool implementations
├── services/     # Business logic
└── tui/          # Terminal UI
```

### Naming Conventions

- **Modules**: `snake_case` (e.g., `llm_provider`)
- **Structs/Enums**: `PascalCase` (e.g., `AgentService`)
- **Functions/Variables**: `snake_case` (e.g., `send_message`)
- **Constants**: `SCREAMING_SNAKE_CASE` (e.g., `DEFAULT_TIMEOUT`)

### Documentation

- **Public APIs**: Must have doc comments
- **Modules**: Should have module-level documentation
- **Complex logic**: Add inline comments explaining "why", not "what"
- **Examples**: Include usage examples in doc comments

Example:
```rust
/// Sends a message to the AI agent and returns the response.
///
/// # Arguments
///
/// * `session_id` - The ID of the conversation session
/// * `message` - The user's message text
/// * `context` - Optional additional context for the AI
///
/// # Returns
///
/// Returns the AI's response including content, token usage, and cost
///
/// # Errors
///
/// Returns an error if:
/// - The session doesn't exist
/// - The provider API call fails
/// - Tool execution fails
///
/// # Example
///
/// ```rust
/// let response = agent.send_message(
///     session_id,
///     "Explain async/await".to_string(),
///     None
/// ).await?;
/// println!("{}", response.content);
/// ```
pub async fn send_message(
    &self,
    session_id: Uuid,
    message: String,
    context: Option<MessageContext>,
) -> Result<AgentResponse> {
    // Implementation...
}
```

## Testing Guidelines

### Test Organization

- **Unit tests**: In the same file as the code, in a `tests` module
- **Integration tests**: In `tests/` directory
- **Test fixtures**: In `tests/fixtures/`

### Writing Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_function_name() {
        // Arrange
        let input = "test";

        // Act
        let result = function_under_test(input);

        // Assert
        assert_eq!(result, expected);
    }

    #[tokio::test]
    async fn test_async_function() {
        // Test async functions
        let result = async_function().await;
        assert!(result.is_ok());
    }
}
```

### Running Tests

```bash
# All tests
cargo test

# Specific test
cargo test test_name

# With output
cargo test -- --nocapture

# Integration tests only
cargo test --test '*'

# Documentation tests
cargo test --doc

# With coverage
cargo tarpaulin --out Html --output-dir coverage
```

### Test Coverage

- Aim for **>80% code coverage** for new features
- **100% coverage** for critical paths (database migrations, security features)
- Use `cargo tarpaulin` to measure coverage

## Documentation

### README Updates

Update README.md if you:
- Add new features
- Change command-line interface
- Add new configuration options
- Change system requirements

### Code Comments

- Explain **why**, not **what**
- Document non-obvious behavior
- Include examples for complex APIs
- Update comments when changing code

### User Guides

If you add a major feature, consider adding a guide in `docs/guides/`:
- Step-by-step instructions
- Screenshots or examples
- Common issues and solutions
- Performance considerations

## Commit Messages

Use clear, descriptive commit messages:

```
Add support for LM Studio local LLMs

- Add OPENAI_BASE_URL environment variable support
- Update CLI to support provider selection
- Add comprehensive LM Studio guide
- Update README with quick start

Fixes #123
```

Format:
- **First line**: Brief summary (50 chars or less)
- **Blank line**
- **Body**: Detailed explanation (wrap at 72 chars)
- **Footer**: Issue references, breaking changes

## License

By contributing to OpenCrab, you agree that your contributions will be licensed under the MIT License. See [LICENSE.md](LICENSE.md) for details.

## Questions?

Don't hesitate to ask questions by:
- Opening an issue with the "question" label
- Starting a discussion in GitHub Discussions
- Reaching out to maintainers

Thank you for contributing to OpenCrab!
