import { initI18n } from './i18n.js';

import('./katex.js');
import('../pkg/index.js').catch(console.error);

initI18n();

// which build is live -- see webpack.config.js's git-sha DefinePlugin
document.getElementById('version-badge').textContent = `v${process.env.GIT_COMMIT_SHA}`;
