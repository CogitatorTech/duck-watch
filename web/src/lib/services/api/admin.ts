import { apiFetch } from '.';
import type { Connection } from './connections';

export type OrganizationOverview = {
	organization: {
		id: string;
		name: string;
		created_at: string;
		updated_at: string;
	};
	user_count: number;
	connections: Connection[];
};

export const listAdminOrganizations = async (fetcher?: typeof fetch) =>
	await apiFetch<OrganizationOverview[]>('/admin/organizations', { method: 'GET' }, fetcher);
