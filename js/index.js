import { initI18n } from './i18n.js';
import { setWasm } from './online.js';

import('./katex.js');
import('../pkg/index.js').then(setWasm).catch(console.error);

initI18n();
