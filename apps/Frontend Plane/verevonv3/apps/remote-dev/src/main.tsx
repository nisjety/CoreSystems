import { render } from '@solidjs/web';
import { App } from './App.js';
import './style.css';

const root = document.getElementById('root');
if (!root) {
  throw new Error('#root element not found');
}

render(() => <App />, root);
