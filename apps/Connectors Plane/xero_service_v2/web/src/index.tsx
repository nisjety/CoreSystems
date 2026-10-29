/* @refresh reload */
import { render } from "solid-js/web";
import { Router, Route } from "@solidjs/router";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";

import "./styles.css";
import { Layout } from "./App";
import Home from "./routes/Home";
import Connected from "./routes/Connected";
import AuthError from "./routes/AuthError";

const queryClient = new QueryClient({
  defaultOptions: {
    queries: { retry: 1, staleTime: 30_000, refetchOnWindowFocus: false },
  },
});

const root = document.getElementById("root");
if (!root) {
  throw new Error("Root element #root not found in index.html");
}

render(
  () => (
    <QueryClientProvider client={queryClient}>
      <Router root={Layout}>
        <Route path="/" component={Home} />
        <Route path="/connected" component={Connected} />
        <Route path="/auth/error" component={AuthError} />
      </Router>
    </QueryClientProvider>
  ),
  root,
);
