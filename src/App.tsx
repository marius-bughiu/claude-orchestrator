import { useEffect } from "react";
import { createHashRouter, RouterProvider, Navigate } from "react-router-dom";
import { Layout } from "./components/Layout";
import { ProjectsView } from "./views/ProjectsView";
import { ProjectDetailView } from "./views/ProjectDetailView";
import { TasksView } from "./views/TasksView";
import { TaskDetailView } from "./views/TaskDetailView";
import { BoardView } from "./views/BoardView";
import { DependencyGraphView } from "./views/DependencyGraphView";
import { PullRequestsView } from "./views/PullRequestsView";
import { ActivityView } from "./views/ActivityView";
import { ScheduledView } from "./views/ScheduledView";
import { DashboardView } from "./views/DashboardView";
import { TimelineView } from "./views/TimelineView";
import { SessionDetailView } from "./views/SessionDetailView";
import { SettingsView } from "./views/SettingsView";
import { SearchView } from "./views/SearchView";
import { useStore } from "./store";

const router = createHashRouter([
  {
    path: "/",
    element: <Layout />,
    children: [
      { index: true, element: <Navigate to="/dashboard" replace /> },
      { path: "dashboard", element: <DashboardView /> },
      { path: "projects", element: <ProjectsView /> },
      { path: "projects/:id", element: <ProjectDetailView /> },
      { path: "tasks", element: <TasksView /> },
      { path: "tasks/:id", element: <TaskDetailView /> },
      { path: "board", element: <BoardView /> },
      { path: "graph", element: <DependencyGraphView /> },
      { path: "pulls", element: <PullRequestsView /> },
      { path: "scheduled", element: <ScheduledView /> },
      { path: "timeline", element: <TimelineView /> },
      { path: "activity", element: <ActivityView /> },
      { path: "search", element: <SearchView /> },
      { path: "sessions/:id", element: <SessionDetailView /> },
      { path: "settings", element: <SettingsView /> },
    ],
  },
]);

export default function App() {
  const init = useStore((s) => s.init);
  useEffect(() => {
    init();
  }, [init]);
  return <RouterProvider router={router} />;
}
