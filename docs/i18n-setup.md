# Internationalization (i18n) Setup

Zaguán Blade uses `next-intl` for internationalization support.

## Structure

```
zblade/
├── messages/
│   └── en.json          # English translations (source of truth)
├── src/
│   └── i18n/
│       └── request.ts   # next-intl configuration
```

## English Source File

The `messages/en.json` file contains all user-facing text in English. This serves as the source of truth for translations.

### Structure

The translations are organized into logical sections:

- **app** - Application name and branding
- **common** - Common UI elements (buttons, actions)
- **editor** - Editor-specific text
- **chat** - AI chat interface
- **fileTree** - File explorer
- **terminal** - Terminal panel
- **tabs** - Tab management
- **diff** - Diff viewer and approval UI
- **approval** - Code change approval workflow
- **settings** - Settings panel
- **statusBar** - Status bar items
- **notifications** - Notification messages
- **search** - Search functionality
- **git** - Source control
- **errors** - Error messages
- **welcome** - Welcome screen
- **commands** - Command palette
- **debug** - Debugger
- **extensions** - Extensions management
- **help** - Help and documentation
- **ai** - AI-specific messages
- **screenshot** - Screenshot capture
- **workspace** - Workspace management
- **language** - Language selection

### Using Translations in Components

```typescript
import { useTranslations } from 'next-intl';

export function MyComponent() {
  const t = useTranslations('chat');
  
  return (
    <div>
      <h1>{t('title')}</h1>
      <button>{t('send')}</button>
    </div>
  );
}
```

### Interpolation

For dynamic values, use curly braces in the JSON:

```json
{
  "editor": {
    "lineCount": "{count} lines"
  }
}
```

Then use it:

```typescript
const t = useTranslations('editor');
t('lineCount', { count: 42 }); // "42 lines"
```

## Future: Adding New Languages

When ready to add new languages, use `tstlai` to translate the English source file:

```bash
# Generate Spanish and French from English source
npx tstlai generate -i messages/en.json -o messages/ -l es,fr

# Generate multiple languages at once
npx tstlai generate -i messages/en.json -o messages/ -l es,fr,de,it,pt

# With context for better translation quality
npx tstlai generate -i messages/en.json -o messages/ -l ja,ko,zh -c "AI-powered code editor"

# Generate all planned languages
npx tstlai generate -i messages/en.json -o messages/ -l es,fr,de,it,pt,ru,zh,ja,ko
```

Then update `src/i18n/request.ts` to support multiple locales:

```typescript
import { getRequestConfig } from 'next-intl/server';

export const locales = ['en', 'es', 'fr'] as const;
export type Locale = (typeof locales)[number];

export default getRequestConfig(async ({ locale }) => {
  return {
    locale,
    messages: (await import(`../../messages/${locale}.json`)).default
  };
});
```

## Translation Guidelines

1. **Keep keys descriptive** - Use clear, hierarchical keys
2. **Use interpolation** - For dynamic values like counts, names, etc.
3. **Be concise** - UI text should be short and clear
4. **Context matters** - Group related translations together
5. **Consistency** - Use consistent terminology across the app

## Supported Languages (Future)

- 🇬🇧 English (en) - **Current**
- 🇪🇸 Spanish (es) - Planned
- 🇫🇷 French (fr) - Planned
- 🇩🇪 German (de) - Planned
- 🇮🇹 Italian (it) - Planned
- 🇵🇹 Portuguese (pt) - Planned
- 🇷🇺 Russian (ru) - Planned
- 🇨🇳 Chinese (zh) - Planned
- 🇯🇵 Japanese (ja) - Planned
- 🇰🇷 Korean (ko) - Planned

## About tstlai

`tstlai` is a TypeScript-first translation tool that:
- Preserves JSON structure
- Maintains interpolation variables
- Provides high-quality translations
- Supports multiple target languages
- Works seamlessly with TypeScript projects

Learn more: https://www.npmjs.com/package/tstlai
