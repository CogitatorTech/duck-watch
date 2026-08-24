import { sveltekit } from '@sveltejs/kit/vite';
import tailwindcss from '@tailwindcss/vite';
// `vitest/config` re-exports Vite's `defineConfig` with the `test` field typed.
import { defineConfig } from 'vitest/config';

export default defineConfig({
	plugins: [tailwindcss(), sveltekit()],
	test: {
		include: ['{src,test}/**/*.{test,spec}.{js,ts}'],
		// The API client reads this at module scope, and `.env` is not in the
		// repository, so without a value here a fresh clone cannot even load
		// the module and its tests silently vanish from the run.
		env: { VITE_API_URL: 'http://localhost:8080/api/v1' },
	},
});
