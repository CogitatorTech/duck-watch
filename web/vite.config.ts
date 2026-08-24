import { sveltekit } from '@sveltejs/kit/vite';
import tailwindcss from '@tailwindcss/vite';
import { defineConfig } from 'vitest/config';

export default defineConfig({
	plugins: [tailwindcss(), sveltekit()],
	test: {
		include: ['{src,test}/**/*.{test,spec}.{js,ts}'],
		env: { VITE_API_URL: 'http://localhost:8080/api/v1' },
	},
});
