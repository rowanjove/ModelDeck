import { useEffect } from "react";
import { Shell } from "./components/Shell";
import { initTaskQueueListener } from "./lib/taskQueue";

export default function App() {
  useEffect(() => {
    void initTaskQueueListener();
  }, []);

  return <Shell />;
}
