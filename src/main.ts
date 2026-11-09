import { createApp } from 'vue';
import App from './App.vue';
import QuickPanel from './QuickPanel.vue';
import './style.css';
import './appearance.css';
import './quick-panel.css';

const quickView = new URLSearchParams(window.location.search).get('view') === 'quick';
document.documentElement.dataset.view = quickView ? 'quick' : 'main';
createApp(quickView ? QuickPanel : App).mount('#app');
