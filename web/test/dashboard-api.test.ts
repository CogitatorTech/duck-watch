import { describe, expect, it } from 'vitest';
import { getFilterValues, getSummary } from '../src/lib/services/api/dashboard';

/** Captures the URL a client builds, without making a request. */
const captureUrl = () => {
	const calls: string[] = [];
	const fetcher = (async (url: string) => {
		calls.push(String(url));
		return new Response('{"user_names":[],"query_types":[]}', {
			status: 200,
			headers: { 'content-type': 'application/json' },
		});
	}) as unknown as typeof fetch;
	return { calls, fetcher };
};

describe('getFilterValues', () => {
	it('asks for the custom range when one is active', async () => {
		const { calls, fetcher } = captureUrl();

		await getFilterValues(
			'c1',
			'24h',
			{ from: '2026-07-01T00:00:00.000Z', to: '2026-07-02T00:00:00.000Z' },
			fetcher,
		);

		// Without this the menus list the preset window's users and types,
		// which need not be the ones present in the selected range.
		expect(calls[0]).toContain('from=2026-07-01T00%3A00%3A00.000Z');
		expect(calls[0]).toContain('to=2026-07-02T00%3A00%3A00.000Z');
	});

	it('falls back to the preset window when no range is given', async () => {
		const { calls, fetcher } = captureUrl();

		await getFilterValues('c1', '7d', {}, fetcher);

		expect(calls[0]).toContain('window=7d');
		expect(calls[0]).not.toContain('from=');
	});

	it('never narrows the menus by the filters they populate', async () => {
		const { calls, fetcher } = captureUrl();

		// A menu narrowed by its own selection would drop every other option
		// and leave no way to change it, so only the range may be sent.
		await getFilterValues(
			'c1',
			'24h',
			{ from: '2026-07-01T00:00:00.000Z' } as Parameters<typeof getFilterValues>[2],
			fetcher,
		);

		for (const excluded of ['user=', 'type=', 'q=', 'min_ms=']) {
			expect(calls[0]).not.toContain(excluded);
		}
	});
});

describe('the shared filter query', () => {
	it('sends the selected query shape', async () => {
		const { calls, fetcher } = captureUrl();

		await getSummary('c1', '24h', { shape: 'abc123' }, fetcher);

		// The backend reads this as the fingerprint filter. Without it a
		// selected shape changes nothing on the page.
		expect(calls[0]).toContain('shape=abc123');
	});

	it('leaves the shape out when nothing is selected', async () => {
		const { calls, fetcher } = captureUrl();

		await getSummary('c1', '24h', {}, fetcher);

		expect(calls[0]).not.toContain('shape=');
	});
});
