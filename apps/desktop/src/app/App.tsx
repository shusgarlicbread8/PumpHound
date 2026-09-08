import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider } from "@tanstack/react-router";
import { useQuery } from "@tanstack/react-query";
import { api } from "../lib/api";
import { Onboarding } from "./Onboarding";
import { router } from "./router";
import { StreamBridge } from "./StreamBridge";

const client = new QueryClient({
  defaultOptions: {
    queries: { retry: 1, refetchOnWindowFocus: false },
  },
});

function Gate() {
  const health = useQuery({
    queryKey: ["health-gate"],
    queryFn: api.health,
    refetchInterval: 2000,
    retry: false,
  });
  if (!health.isSuccess || !health.data.ready) {
    return <Onboarding />;
  }
  return (
    <>
      <StreamBridge />
      <RouterProvider router={router} />
    </>
  );
}

export function App() {
  return (
    <QueryClientProvider client={client}>
      <Gate />
    </QueryClientProvider>
  );
}
