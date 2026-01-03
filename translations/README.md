# Translation Files

This directory contains all translation files for the Rustrade application.

## 🚀 Adding a New Language (Zero Code Changes!)

To add support for a new language, simply:

1. **Copy an existing translation file** (e.g., `en.json`)
2. **Rename it** with the appropriate language code (e.g., `es.json` for Spanish, `de.json` for German, `ja.json` for Japanese)
3. **Update the language metadata** at the top of the file:
   ```json
   {
     "language": {
       "code": "es",
       "name": "Spanish",
       "flag": "🇪🇸",
       "native_name": "Español"
     },
     ...
   }
   ```
4. **Translate all the values** (keeping the same keys)
5. **Save the file** - The application will automatically detect and load it!

**That's it!** No Rust code modifications needed. The language will appear automatically in the language selector.

## 📁 Translation File Format

Each translation file must contain:

### 1. Language Metadata (Required)
```json
{
  "language": {
    "code": "xx",           // ISO 639-1 code (2 letters)
    "name": "Language",     // English name
    "flag": "🏳️",           // Flag emoji
    "native_name": "Native" // Name in native language
  },
  ...
}
```

### 2. UI Translations
```json
{
  ...
  "ui": {
    "help_panel_title": "...",
    "help_button_label": "...",
    ...
  },
  ...
}
```

### 3. Help Categories
```json
{
  ...
  "help_categories": {
    "abbreviations": "...",
    "strategies": "...",
    ...
  },
  ...
}
```

### 4. Help Topics
```json
{
  ...
  "help_topics": [
    {
      "id": "unique_id",
      "category": "abbreviations",
      "title": "Short Title",
      "abbreviation": "Optional",
      "full_name": "Full Descriptive Name",
      "description": "Detailed explanation...",
      "example": "Practical example..."
    },
    ...
  ]
}
```

## 🌍 Currently Supported Languages

The application automatically discovers all `.json` files in this directory:

- 🇫🇷 French (`fr.json`)
- 🇬🇧 English (`en.json`)

**Want to contribute a translation?** Just add your `.json` file and submit a PR!

## ✅ Testing Your Translation

1. Place your `xx.json` file in the `translations/` directory
2. Run the application: `cargo run --bin rustrade`
3. Open the language selector (if UI is implemented)
4. Your language should appear automatically in the list
5. Select it to test all translations

## 💡 Tips for Translators

- **Keep keys identical** to those in `en.json`
- **Use appropriate currency symbols** for your locale (€, $, ¥, etc.)
- **Adapt examples** to make sense in your language  and culture
- **Financial terminology** should be accurate for your target market
- **Test thoroughly** - especially numeric formats and date displays

## 🔧 Technical Details

- **Auto-Discovery**: The Rust code scans this directory at startup
- **No Compilation**: Adding/modifying translations doesn't require rebuilding
- **Fallback**: Missing translations fall back to the key name
- **Format**: Standard JSON with UTF-8 encoding

## 🤝 Contributing

Native speakers are welcome to contribute translations! Please ensure:
- All required keys from `en.json` are present
- Translations are accurate and natural-sounding
- Financial terminology is correct for the target locale
- Examples use appropriate currency symbols and formats
- The JSON file is valid (use a JSON validator)

Thank you for helping make Rustrade accessible worldwide! 🌏
