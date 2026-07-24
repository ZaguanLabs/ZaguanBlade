import { PrismLight } from 'react-syntax-highlighter';
import bash from 'react-syntax-highlighter/dist/esm/languages/prism/bash';
import cpp from 'react-syntax-highlighter/dist/esm/languages/prism/cpp';
import css from 'react-syntax-highlighter/dist/esm/languages/prism/css';
import go from 'react-syntax-highlighter/dist/esm/languages/prism/go';
import javascript from 'react-syntax-highlighter/dist/esm/languages/prism/javascript';
import json from 'react-syntax-highlighter/dist/esm/languages/prism/json';
import markdown from 'react-syntax-highlighter/dist/esm/languages/prism/markdown';
import markup from 'react-syntax-highlighter/dist/esm/languages/prism/markup';
import php from 'react-syntax-highlighter/dist/esm/languages/prism/php';
import python from 'react-syntax-highlighter/dist/esm/languages/prism/python';
import rust from 'react-syntax-highlighter/dist/esm/languages/prism/rust';
import typescript from 'react-syntax-highlighter/dist/esm/languages/prism/typescript';
import yaml from 'react-syntax-highlighter/dist/esm/languages/prism/yaml';

const languages = [
    ['bash', bash],
    ['shell', bash],
    ['sh', bash],
    ['cpp', cpp],
    ['css', css],
    ['go', go],
    ['javascript', javascript],
    ['js', javascript],
    ['jsx', javascript],
    ['json', json],
    ['markdown', markdown],
    ['md', markdown],
    ['html', markup],
    ['markup', markup],
    ['php', php],
    ['python', python],
    ['py', python],
    ['rust', rust],
    ['rs', rust],
    ['typescript', typescript],
    ['ts', typescript],
    ['tsx', typescript],
    ['yaml', yaml],
    ['yml', yaml],
] as const;

for (const [name, grammar] of languages) {
    PrismLight.registerLanguage(name, grammar);
}

export { PrismLight as SyntaxHighlighter };
