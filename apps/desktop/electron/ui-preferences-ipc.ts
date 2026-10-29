// Wires the client's UI preferences store (ui-preferences.ts) into Electron:
// every app window hears about a change, whoever made it.
import { app, BrowserWindow, ipcMain } from 'electron'

import { registerUiPreferencesIpc } from './ui-preferences'

/** Client presentation belongs to this app, independent of the connected agent. */
export function registerMachineUiPreferences(): () => void {
  return registerUiPreferencesIpc({
    ipcMain,
    userDataDir: app.getPath('userData'),
    broadcast: snapshot => {
      for (const window of BrowserWindow.getAllWindows()) {
        window.webContents.send('hermes:ui-preferences:changed', snapshot)
      }
    }
  })
}
