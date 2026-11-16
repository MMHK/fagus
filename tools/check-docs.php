<?php

declare(strict_types=1);

/*
 * Checks the documentation in docs/, which fagus.niklasforst.de/docs renders from this repository:
 *
 *     php tools/check-docs.php
 *
 * - docs/nav.yml has one entry per Markdown file in docs/, in the line format the website reads
 *   (see the comment at the top of nav.yml), with known fields and short search engine texts;
 * - links between the pages lead to pages and headings that exist, and links to other files of
 *   the repository to files that exist.
 *
 * Heading anchors are made as GitHub and the website make them: the text of the heading in lower
 * case, without punctuation except hyphens and underscores, spaces replaced by hyphens, and "-1",
 * "-2" … for repeated headings.
 */

$root = dirname(__DIR__);
$docs = $root.'/docs';
$errors = [];

// The pages: every Markdown file below docs/, by its path without .md ("adr/README").
$pages = [];
$files = new RecursiveIteratorIterator(new RecursiveDirectoryIterator($docs, FilesystemIterator::SKIP_DOTS));
foreach ($files as $file) {
    if ($file->isFile() && $file->getExtension() === 'md') {
        $pages[] = substr(str_replace('\\', '/', $file->getPathname()), strlen($docs) + 1, -3);
    }
}
sort($pages);

// docs/nav.yml
$value = '(?:"(?:[^"\\\\]|\\\\.)*"|[^,}"]+)';
$entryPattern = "/^- \\{ page: ([\\w\\/-]+), title: ({$value})((?:, \\w+: {$value})*) \\}$/";
$unquote = static fn (string $raw): string => str_starts_with(trim($raw), '"') ? (string) json_decode(trim($raw)) : trim($raw);
$entries = [];
$source = file_get_contents($docs.'/nav.yml');
if ($source === false) {
    $errors[] = 'docs/nav.yml is missing';
    $source = '';
}
foreach (explode("\n", $source) as $number => $line) {
    if ($line === '' || str_starts_with($line, '#')) {
        continue;
    }
    if (preg_match($entryPattern, $line, $match) !== 1) {
        $errors[] = 'docs/nav.yml:'.($number + 1).': not in the form "- { page: …, title: …, field: value, … }"';

        continue;
    }
    preg_match_all("/, (\\w+): ({$value})/", $match[3], $fields, PREG_SET_ORDER);
    $entries[] = [
        'line' => $number + 1,
        'page' => $match[1],
        'title' => $unquote($match[2]),
        'fields' => array_column(array_map(static fn (array $field): array => [$field[1], $unquote($field[2])], $fields), 1, 0),
    ];
}

$listed = array_column($entries, 'page');
foreach (array_diff($pages, $listed) as $page) {
    $errors[] = "docs/{$page}.md has no entry in docs/nav.yml";
}
foreach (array_diff($listed, $pages) as $page) {
    $errors[] = "docs/nav.yml lists {$page}, but docs/{$page}.md does not exist";
}
foreach (array_diff_assoc($listed, array_unique($listed)) as $page) {
    $errors[] = "docs/nav.yml lists {$page} twice";
}
if (($listed[0] ?? null) !== 'README') {
    $errors[] = 'docs/nav.yml must start with README, the page of /docs';
}

$titles = array_column($entries, 'title');
foreach (array_diff_assoc($titles, array_unique($titles)) as $title) {
    $errors[] = "docs/nav.yml has the title \"{$title}\" twice";
}
$groups = [];
foreach ($entries as $entry) {
    $at = "docs/nav.yml:{$entry['line']} ({$entry['page']})";
    foreach (array_keys($entry['fields']) as $field) {
        if (! in_array($field, ['group', 'parent', 'optional', 'seo', 'description'], true)) {
            $errors[] = "{$at}: unknown field {$field}";
        }
    }
    $group = $entry['fields']['group'] ?? null;
    if ($group === null) {
        $errors[] = "{$at}: no group";
    } elseif ($groups !== [] && end($groups) !== $group && in_array($group, $groups, true)) {
        $errors[] = "{$at}: the pages of the group {$group} must follow each other";
    }
    if ($group !== null && end($groups) !== $group) {
        $groups[] = $group;
    }
    if (isset($entry['fields']['parent']) && ! in_array($entry['fields']['parent'], $titles, true)) {
        $errors[] = "{$at}: no page has the parent's title \"{$entry['fields']['parent']}\"";
    }
    if (isset($entry['fields']['optional']) && $entry['fields']['optional'] !== 'true') {
        $errors[] = "{$at}: optional is either true or left out";
    }
    if (isset($entry['fields']['seo']) && mb_strlen($entry['fields']['seo']) > 60) {
        $errors[] = "{$at}: seo is longer than 60 characters";
    }
    if (isset($entry['fields']['description']) && (mb_strlen($entry['fields']['description']) < 70 || mb_strlen($entry['fields']['description']) > 160)) {
        $errors[] = "{$at}: description should have 70 to 160 characters, not ".mb_strlen($entry['fields']['description']);
    }
}

/** Markdown without fenced code blocks and code spans, so that examples are not taken for links. */
function prose(string $markdown): string
{
    $markdown = (string) preg_replace('/^[ \t]*(`{3,}|~{3,})[^\n]*\n.*?^[ \t]*\1[ \t]*$/ms', '', $markdown);

    return (string) preg_replace('/`[^`\n]*`/', '', $markdown);
}

/**
 * The anchors of the headings of a Markdown file, as GitHub and the website make them.
 *
 * @return list<string>
 */
function anchors(string $markdown): array
{
    $markdown = (string) preg_replace('/^[ \t]*(`{3,}|~{3,})[^\n]*\n.*?^[ \t]*\1[ \t]*$/ms', '', $markdown);
    preg_match_all('/^#{1,6}[ \t]+(.+?)[ \t]*#*[ \t]*$/m', $markdown, $headings);
    $anchors = [];
    $seen = [];
    foreach ($headings[1] as $heading) {
        $text = (string) preg_replace('/!?\[([^\]]*)\]\([^)]*\)/', '$1', $heading);
        $text = (string) preg_replace('/`([^`]*)`/', '$1', $text);
        $text = (string) preg_replace('/\*\*([^*]+)\*\*|\*([^*]+)\*/', '$1$2', $text);
        $id = str_replace(' ', '-', (string) preg_replace('/[^\p{L}\p{M}\p{N}\p{Pc}\- ]/u', '', mb_strtolower(trim($text))));
        $anchors[] = isset($seen[$id]) ? $id.'-'.$seen[$id] : $id;
        $seen[$id] = ($seen[$id] ?? 0) + 1;
    }

    return $anchors;
}

/** A path relative to the repository, with "." and ".." resolved; null if it leaves the repository. */
function normalize(string $path): ?string
{
    $parts = [];
    foreach (explode('/', $path) as $part) {
        if ($part === '..') {
            if ($parts === []) {
                return null;
            }
            array_pop($parts);
        } elseif ($part !== '.' && $part !== '') {
            $parts[] = $part;
        }
    }

    return implode('/', $parts);
}

// Links of the pages: [text](target), also with the text over several lines, and href="target".
$anchorCache = [];
foreach ($pages as $page) {
    $file = "docs/{$page}.md";
    $markdown = prose((string) file_get_contents("{$root}/{$file}"));
    preg_match_all('/\]\(\s*<?([^)\s>]+)>?(?:\s+"[^"]*")?\s*\)|\bhref="([^"]+)"/', $markdown, $links, PREG_SET_ORDER);
    foreach ($links as $link) {
        $target = ($link[1] ?? '') !== '' ? $link[1] : $link[2];
        if (preg_match('{^([a-z][a-z0-9+.-]*:|//)}i', $target) === 1) {
            continue;
        }
        [$path, $anchor] = array_pad(explode('#', $target, 2), 2, '');
        $resolved = $path === '' ? $file : normalize(dirname($file).'/'.rawurldecode($path));
        if ($resolved === null || ! file_exists("{$root}/{$resolved}")) {
            $errors[] = "{$file}: the link {$target} leads to a file that does not exist";

            continue;
        }
        if (is_dir("{$root}/{$resolved}")) {
            if (str_starts_with($resolved.'/', 'docs/') && ! is_file("{$root}/{$resolved}/README.md")) {
                $errors[] = "{$file}: the link {$target} leads to a folder of docs/ without a README.md";
            }
            $resolved .= '/README.md';
            if (! is_file("{$root}/{$resolved}")) {
                continue;
            }
        }
        if ($anchor !== '' && str_ends_with($resolved, '.md')) {
            $anchorCache[$resolved] ??= anchors((string) file_get_contents("{$root}/{$resolved}"));
            if (! in_array($anchor, $anchorCache[$resolved], true)) {
                $errors[] = "{$file}: the link {$target} leads to a heading that does not exist";
            }
        }
    }
}

if ($errors !== []) {
    fwrite(STDERR, implode("\n", $errors)."\n");
    exit(1);
}
echo count($pages)." pages in docs/nav.yml, links between them resolve.\n";
