import { describe, expect, it } from 'vitest';
import type { ConnectionStatus, IngestionHealth } from '../src/lib/services/api/connections';
import { describeHealth, formatAge, healthTone } from '../src/lib/services/health';

const status = (health: IngestionHealth, overrides: Partial<ConnectionStatus> = {}) =>
	({
		id: 'c1',
		org_id: 'o1',
		name: 'prod',
		region_tier: 'tier1',
		enabled: health !== 'disabled',
		watermark_start_time: null,
		last_synced_at: '2026-08-23T10:00:00Z',
		last_success_at: '2026-08-23T10:00:00Z',
		last_sync_error: null,
		created_at: '2026-08-01T00:00:00Z',
		updated_at: '2026-08-23T10:00:00Z',
		health,
		seconds_since_success: 30,
		seconds_behind: 90,
		stale_after_seconds: 300,
		...overrides,
	}) satisfies ConnectionStatus;

describe('formatAge', () => {
	it('keeps seconds under a minute', () => {
		expect(formatAge(45)).toBe('45s');
	});

	it('switches to minutes, hours, and days as the age grows', () => {
		expect(formatAge(120)).toBe('2 min');
		expect(formatAge(7200)).toBe('2 hours');
		expect(formatAge(172800)).toBe('2 days');
	});

	it('keeps the unit singular for exactly one', () => {
		expect(formatAge(3600)).toBe('1 hour');
		expect(formatAge(86400)).toBe('1 day');
	});

	it('never reports a negative age from a clock that runs ahead', () => {
		expect(formatAge(-10)).toBe('0s');
	});
});

describe('describeHealth', () => {
	it('says nothing alarming about a healthy connection', () => {
		const notice = describeHealth(status('healthy'));
		expect(notice.tone).toBe('ok');
		expect(notice.consequence).toBe('');
		expect(notice.error).toBeNull();
		expect(notice.detail).toContain('30s');
	});

	it('reports how far behind the newest ingested query is', () => {
		expect(describeHealth(status('healthy')).detail).toContain('2 min');
	});

	it('warns that a stale connection makes the figures too low', () => {
		const notice = describeHealth(
			status('stale', { seconds_since_success: 7200, last_sync_error: null }),
		);
		expect(notice.tone).toBe('warn');
		expect(notice.detail).toContain('2 hours');
		// The threshold is named, so the judgment is not a bare assertion.
		expect(notice.detail).toContain('5 min');
		expect(notice.consequence).toContain('too low');
	});

	it('surfaces the reported error on a failing connection', () => {
		const notice = describeHealth(
			status('failing', {
				seconds_since_success: 86400,
				last_sync_error: 'permission denied on query_history',
			}),
		);
		expect(notice.tone).toBe('danger');
		expect(notice.error).toBe('permission denied on query_history');
		expect(notice.consequence).toContain('too low');
	});

	it('does not claim an age for a connection that never succeeded', () => {
		const notice = describeHealth(
			status('failing', { seconds_since_success: null, last_success_at: null }),
		);
		expect(notice.detail).toContain('failed');
		expect(notice.detail).not.toContain('null');
	});

	it('treats a pending connection as neither broken nor healthy', () => {
		const notice = describeHealth(
			status('pending', { seconds_since_success: null, last_synced_at: null }),
		);
		expect(notice.tone).toBe('neutral');
		expect(notice.consequence).not.toBe('');
	});

	it('explains that a disabled connection stops receiving data', () => {
		const notice = describeHealth(status('disabled'));
		expect(notice.tone).toBe('neutral');
		expect(notice.consequence).toContain('No new data will arrive');
	});
});

describe('healthTone', () => {
	it('maps each state to a tone', () => {
		expect(healthTone('healthy')).toBe('ok');
		expect(healthTone('stale')).toBe('warn');
		expect(healthTone('failing')).toBe('danger');
		expect(healthTone('pending')).toBe('neutral');
		expect(healthTone('disabled')).toBe('neutral');
	});
});
