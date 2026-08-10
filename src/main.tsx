/**
 * Frontend entry point: the first application code the webview executes.
 *
 * Responsibilities are deliberately minimal -- install global error reporting,
 * then mount the React tree. Everything else (routing, providers, layout) is
 * App's concern.
 */
import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
import './App.css';
import { installGlobalErrorHandlers } from '@/lib/rendererErrorReporting';

// Installed before React mounts so that a failure during the very first render,
// or in any module evaluated after this point, is still captured and forwarded
// to the backend rather than vanishing into the webview console.
installGlobalErrorHandlers();

// StrictMode double-invokes renders and effects in development to surface
// impure logic early; it has no effect on the production build.
ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
