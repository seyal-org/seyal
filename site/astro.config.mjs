import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

export default defineConfig({
  integrations: [
    starlight({
      title: 'Seyal Docs',
      description: 'User and developer documentation for the Seyal terminal workspace.',
      social: [{ icon: 'github', label: 'GitHub', href: 'https://github.com/mahboobmonnamd/seyal' }],
      sidebar: [
        {
          label: 'User Guide',
          items: [
            { label: 'Overview', slug: 'user' },
            { label: 'Getting Started', slug: 'user/getting-started' },
            { label: 'What is available now?', slug: 'user/current-status' }
          ]
        },
        {
          label: 'Developer Guide',
          items: [
            { label: 'Overview', slug: 'developer' },
            { label: 'Architecture', slug: 'developer/architecture' },
            { label: 'Local Context Engine', slug: 'developer/local-context-engine' },
            { label: 'Contributing', slug: 'developer/contributing' }
          ]
        }
      ]
    })
  ]
});
