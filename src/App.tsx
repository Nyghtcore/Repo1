import { useEffect, useState } from "react";

import { ensureNotificationPermission } from "./api";
import { isMobile } from "./platform";

import { MediaPane } from "./components/MediaPane";
import { NotesPane } from "./components/NotesPane";
import { PasswordsPane } from "./components/PasswordsPane";
import { RemindersPane } from "./components/RemindersPane";
import { Sidebar, VaultPane, type Section } from "./components/Sidebar";
import { TasksPane } from "./components/TasksPane";
import { UnlockScreen } from "./components/UnlockScreen";
import { ToastProvider, ToastViewport } from "./context/ToastContext";
import { VaultProvider, useVault } from "./context/VaultContext";

function Workspace() {
  const [section, setSection] = useState<Section>("passwords");

  // Android 13+ needs an explicit grant before reminder notifications can show.
  useEffect(() => {
    if (isMobile) void ensureNotificationPermission();
  }, []);
  return (
    <div className="workspace">
      <Sidebar section={section} onSelect={setSection} />
      <main className="workspace-main">
        {section === "passwords" ? <PasswordsPane /> : null}
        {section === "notes" ? <NotesPane /> : null}
        {section === "tasks" ? <TasksPane /> : null}
        {section === "media" ? <MediaPane /> : null}
        {section === "reminders" ? <RemindersPane /> : null}
        {section === "vault" ? <VaultPane /> : null}
      </main>
    </div>
  );
}

function Shell() {
  const { vault } = useVault();
  return vault && vault.unlocked ? <Workspace /> : <UnlockScreen />;
}

export default function App() {
  return (
    <ToastProvider>
      <VaultProvider>
        <Shell />
        <ToastViewport />
      </VaultProvider>
    </ToastProvider>
  );
}
