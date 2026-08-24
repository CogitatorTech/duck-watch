export type SqlTokenKind = 'keyword' | 'string' | 'number' | 'comment' | 'plain';

export type SqlToken = { text: string; kind: SqlTokenKind };

// Common SQL and DuckDB keywords; enough for readable coloring, not a parser.
const KEYWORDS = new Set([
	'all',
	'alter',
	'and',
	'any',
	'as',
	'asc',
	'between',
	'by',
	'case',
	'cast',
	'create',
	'cross',
	'delete',
	'desc',
	'distinct',
	'drop',
	'else',
	'end',
	'except',
	'exists',
	'filter',
	'from',
	'full',
	'group',
	'having',
	'ilike',
	'in',
	'inner',
	'insert',
	'intersect',
	'interval',
	'into',
	'is',
	'join',
	'lateral',
	'left',
	'like',
	'limit',
	'not',
	'null',
	'offset',
	'on',
	'or',
	'order',
	'outer',
	'over',
	'partition',
	'pivot',
	'qualify',
	'replace',
	'right',
	'select',
	'set',
	'table',
	'then',
	'union',
	'unnest',
	'unpivot',
	'update',
	'using',
	'values',
	'view',
	'when',
	'where',
	'window',
	'with',
]);

// One alternative per token class; longest matches (comments, strings) first.
// Unterminated strings and block comments still match, since previews can cut
// a statement anywhere.
const TOKEN_PATTERN =
	/(--[^\n]*|\/\*[\s\S]*?(?:\*\/|$))|('(?:[^']|'')*'?)|(\b\d+(?:\.\d+)?\b)|([A-Za-z_][A-Za-z0-9_]*)|("(?:[^"]|"")*"?)|([\s\S])/g;

/** Splits SQL text into colorable tokens; concatenating them restores the input. */
export const tokenizeSql = (sql: string): SqlToken[] => {
	const tokens: SqlToken[] = [];

	const push = (text: string, kind: SqlTokenKind) => {
		const last = tokens.at(-1);
		if (last && last.kind === kind) last.text += text;
		else tokens.push({ text, kind });
	};

	for (const match of sql.matchAll(TOKEN_PATTERN)) {
		const [text, comment, string, number, word] = match;
		if (comment !== undefined) push(text, 'comment');
		else if (string !== undefined) push(text, 'string');
		else if (number !== undefined) push(text, 'number');
		else if (word !== undefined) {
			push(text, KEYWORDS.has(word.toLowerCase()) ? 'keyword' : 'plain');
		} else push(text, 'plain');
	}

	return tokens;
};

/**
 * The part of a statement worth showing in one line of a table. Leading
 * comments and blank lines are skipped, because a preview of `-- v2 counting
 * rule ...` looks identical for every query in a family and says nothing
 * about what the query does.
 */
export const previewSql = (sql: string): string => {
	const lines = sql.split('\n');
	const start = lines.findIndex((line) => {
		const trimmed = line.trim();
		return trimmed !== '' && !trimmed.startsWith('--');
	});

	// A statement that really is nothing but comments still deserves a preview.
	const body = start === -1 ? sql : lines.slice(start).join('\n');
	return body.replace(/\s+/g, ' ').trim();
};
