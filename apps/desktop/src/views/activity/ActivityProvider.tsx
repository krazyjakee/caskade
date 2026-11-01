import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react";
import { type InstallApi, type JobSnapshot, mergeJobs } from "../../api/install";
import { libraryKeys } from "../../api/provider";
import type { StorageApi } from "../../api/storage";
import { effectSubscription } from "../../api/subscription";

const Context = createContext<InstallApi | null>(null);
const StorageContext = createContext<StorageApi | undefined>(undefined);
/** Whether job events arrive; without them the list is polled. */
const LiveContext = createContext(false);
/** Jobs whose questions open wherever the user is (`JobQuestionHost`). */
const FollowContext = createContext<{
  followed: string[];
  follow: (jobId: string) => void;
  unfollow: (jobId: string) => void;
} | null>(null);
export const activityKey = ["activity", "jobs"] as const;

export function ActivityProvider({
  api,
  storageApi,
  children,
}: {
  api: InstallApi;
  storageApi?: StorageApi;
  children: ReactNode;
}) {
  const client = useQueryClient();
  const [live, setLive] = useState(false);
  const [followed, setFollowed] = useState<string[]>([]);
  const follow = useCallback(
    (jobId: string) =>
      setFollowed((current) => (current.includes(jobId) ? current : [...current, jobId])),
    [],
  );
  const unfollow = useCallback(
    (jobId: string) => setFollowed((current) => current.filter((id) => id !== jobId)),
    [],
  );
  const following = useMemo(() => ({ followed, follow, unfollow }), [followed, follow, unfollow]);
  useEffect(
    () =>
      effectSubscription(
        api.subscribe,
        (job) => {
          client.setQueryData<JobSnapshot[]>(activityKey, (current) => mergeJobs(current, [job]));
          if (job.status === "completed")
            void client.invalidateQueries({ queryKey: libraryKeys.all });
        },
        { subscribed: () => setLive(true) },
      ),
    [api, client],
  );
  return (
    <Context.Provider value={api}>
      <LiveContext.Provider value={live}>
        <StorageContext.Provider value={storageApi}>
          <FollowContext.Provider value={following}>{children}</FollowContext.Provider>
        </StorageContext.Provider>
      </LiveContext.Provider>
    </Context.Provider>
  );
}
export const useActivityStorage = () => useContext(StorageContext);

export function useInstallApi() {
  const api = useContext(Context);
  if (!api) throw new Error("ActivityProvider is required");
  return api;
}
export function useFollowing() {
  const following = useContext(FollowContext);
  if (!following) throw new Error("ActivityProvider is required");
  return following;
}
const ignore = () => {};
/** Opens the job's question wherever the user is: now if it waits for one,
 * else once it asks, and each one after it until the job ends or the user
 * puts it off. */
export const useFollowJob = () => useContext(FollowContext)?.follow ?? ignore;

const settled = new Set<JobSnapshot["status"]>(["completed", "failed", "cancelled"]);

export function useJobs() {
  const api = useInstallApi();
  const live = useContext(LiveContext);
  const client = useQueryClient();
  return useQuery({
    queryKey: activityKey,
    queryFn: async () =>
      mergeJobs(client.getQueryData<JobSnapshot[]>(activityKey), await api.listJobs()),
    staleTime: 5_000,
    // Job events keep the list current; polling backs them up while a job
    // runs, and replaces them when they never connected.
    refetchInterval: (query) =>
      !live || query.state.data?.some((job) => !settled.has(job.status)) ? 5_000 : false,
    retry: 1,
  });
}
export function useJobActions() {
  const api = useInstallApi();
  const client = useQueryClient();
  return useMutation({
    mutationFn: async (
      action:
        | { type: "cancel"; jobId: string }
        | { type: "retry"; jobId: string; strategyId?: string }
        | { type: "answer"; jobId: string; questionId: string; choiceId: string },
    ) => {
      if (action.type === "cancel") await api.cancel(action.jobId);
      else if (action.type === "answer")
        await api.answer(action.jobId, action.questionId, action.choiceId);
      else {
        const job = await api.retry(action.jobId, action.strategyId);
        client.setQueryData<JobSnapshot[]>(activityKey, (current) => mergeJobs(current, [job]));
      }
    },
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: activityKey });
    },
  });
}
