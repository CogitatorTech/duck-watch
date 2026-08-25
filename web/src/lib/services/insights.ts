import type { Antipattern, Insight } from '$lib/services/api/dashboard';
import { formatBytes } from '$lib/services/chart';

export type InsightCopy = {
	/** What was found, as a short noun phrase. */
	title: string;
	/** Why it is worth reviewing, in one sentence. */
	explanation: string;
	/** What to try, stated as a suggestion rather than an instruction. */
	suggestion: string;
};

const COPY: Record<Antipattern, InsightCopy> = {
	select_star: {
		title: 'Reads every column',
		explanation:
			'MotherDuck stores data in columns and reads only the ones a query names. Using select * reads them all.',
		suggestion: 'Listing only the columns you use will read less data.',
	},
	cross_join: {
		title: 'Join without a condition',
		explanation:
			'A join with no condition pairs every row on one side with every row on the other. Two tables of a thousand rows each make a million.',
		suggestion: 'Check whether a join condition was meant to be here.',
	},
	no_filter: {
		title: 'Scans the whole table',
		explanation:
			'The query reads a table with no where clause, so it reads every row. That gets slower as the table grows.',
		suggestion:
			'Adding a where clause, on a date column for example, means there is less to read.',
	},
	order_without_limit: {
		title: 'Sorts without a limit',
		explanation:
			'Sorting with no limit puts the whole result in order, including the rows nobody looks at.',
		suggestion: 'If you only need the top rows, add a limit.',
	},
	spilling: {
		title: 'Ran out of memory',
		explanation:
			'The Duckling ran out of memory, so these runs wrote working data to disk. Disk is slower than memory.',
		suggestion:
			'A bigger Duckling size, or a query that reads less, would keep the work in memory.',
	},
	repeated_runs: {
		title: 'Run repeatedly',
		explanation: 'The same query ran many times, and each run paid for the same work again.',
		suggestion:
			'If the data changes less often than the query runs, saving the result to a table would avoid repeating the work.',
	},
};

export const insightCopy = (antipattern: Antipattern): InsightCopy => COPY[antipattern];

/**
 * The measurement behind one finding, so a reader can check it rather than
 * take it on trust.
 */
export const insightEvidence = (insight: Insight): string => {
	switch (insight.antipattern) {
		case 'spilling':
			return `${formatBytes(insight.bytes_spilled)} written to disk across ${insight.runs} ${
				insight.runs === 1 ? 'run' : 'runs'
			}`;
		case 'repeated_runs':
			return `${insight.runs} runs in this period`;
		default:
			return `${insight.runs} ${insight.runs === 1 ? 'run' : 'runs'} in this period`;
	}
};

/**
 * How much of a statement the backend keeps in a shape listing. It cuts at
 * this many characters and appends an ellipsis, so anything longer than this
 * has been cut.
 */
export const STATEMENT_LIMIT = 2000;

export const isStatementCut = (sql: string): boolean => sql.length > STATEMENT_LIMIT;

/**
 * The statement to put on the clipboard. A cut statement says so in a SQL
 * comment, because handing someone an incomplete query they believe is whole
 * is worse than handing them nothing.
 */
export const statementForCopy = (sql: string): string =>
	isStatementCut(sql)
		? `${sql}\n-- This query was cut at ${STATEMENT_LIMIT} characters and is not complete.`
		: sql;

/**
 * Every finding raised against one query shape. The backend reports findings
 * one per reason, so the same statement can come back two or three times.
 * Reading it that way means the same SQL, evidence, and buttons repeat down
 * the page, so they are gathered here into one entry per query.
 */
export type ShapeFindings = {
	fingerprint: string;
	example_sql: string;
	estimated_cost_usd: number;
	cost_share: number;
	runs: number;
	bytes_spilled: number;
	last_seen: string;
	/** Every reason this shape was flagged, in the order they were raised. */
	reasons: Insight[];
};

/**
 * Gathers findings by the shape they describe, keeping the order the backend
 * chose. A shape appears where its first, and so its dearest, finding did.
 */
export const groupByShape = (findings: Insight[]): ShapeFindings[] => {
	const order: string[] = [];
	const byShape = new Map<string, ShapeFindings>();

	for (const finding of findings) {
		const existing = byShape.get(finding.fingerprint);
		if (existing) {
			existing.reasons.push(finding);
			continue;
		}
		order.push(finding.fingerprint);
		byShape.set(finding.fingerprint, {
			fingerprint: finding.fingerprint,
			example_sql: finding.example_sql,
			estimated_cost_usd: finding.estimated_cost_usd,
			cost_share: finding.cost_share,
			runs: finding.runs,
			bytes_spilled: finding.bytes_spilled,
			last_seen: finding.last_seen,
			reasons: [finding],
		});
	}

	return order.map((fingerprint) => byShape.get(fingerprint) as ShapeFindings);
};

/**
 * What was measured about one query, as opposed to about one reason. Runs and
 * spilled bytes belong to the query, so they are said once rather than
 * repeated in every reason beside it.
 */
export const shapeEvidence = (shape: ShapeFindings): string => {
	const runs = `${shape.runs} ${shape.runs === 1 ? 'run' : 'runs'}`;
	return shape.bytes_spilled > 0
		? `${runs} · ${formatBytes(shape.bytes_spilled)} written to disk`
		: runs;
};

/**
 * One query shape and everything found about it, as plain text for pasting
 * into a message or a ticket. It is usually passed to whoever owns the query,
 * so it carries the reasons and the numbers as well as the statement. The
 * caller supplies the statement, since the whole one is read separately from
 * the listing.
 */
export const findingAsText = (shape: ShapeFindings, cost: string, statement: string): string => {
	const share = `${(shape.cost_share * 100).toFixed(1)}% of the period`;
	const lines = [
		`Query shape: ${shape.fingerprint}`,
		`Cost: ${cost} (${share})`,
		`Runs: ${shape.runs}`,
		'',
	];
	for (const reason of shape.reasons) {
		const copy = insightCopy(reason.antipattern);
		lines.push(`${copy.title}`);
		lines.push(`  Evidence: ${insightEvidence(reason)}`);
		lines.push(`  Why: ${copy.explanation}`);
		lines.push(`  Try: ${copy.suggestion}`);
		lines.push('');
	}
	lines.push(statementForCopy(statement));
	return lines.join('\n');
};
