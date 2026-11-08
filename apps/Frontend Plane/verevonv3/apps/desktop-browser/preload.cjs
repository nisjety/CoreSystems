const { contextBridge, ipcRenderer } = require('electron');

const channel = 'verevon:desktop-browser:';
const invoke = (method, ...args) => ipcRenderer.invoke(`${channel}${method}`, ...args);

if (process.isMainFrame) {
  contextBridge.exposeInMainWorld('verevonDesktopBrowser', {
    getState: () => invoke('getState'),
    setViewport: (bounds) => invoke('setViewport', bounds),
    navigate: (url) => invoke('navigate', url),
    command: (command) => invoke('command', command),
    startRun: () => invoke('startRun'),
    execute: (runId, action) => invoke('execute', runId, action),
    endRun: (runId) => invoke('endRun', runId),
    reset: () => invoke('reset'),
    subscribe: (listener) => {
      const receive = (_event, state) => listener(state);
      ipcRenderer.on(`${channel}event`, receive);
      return () => ipcRenderer.removeListener(`${channel}event`, receive);
    },
  });
}
