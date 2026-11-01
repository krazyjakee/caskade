import { useEffect, useState } from "react";
import { useFollowing, useJobActions, useJobs } from "./ActivityProvider";
import { QuestionDialog } from "./QuestionDialog";

/** The question of a followed job (`useFollowJob`), over whatever view is
 * open: a job the user started takes them to its next step. Not now leaves
 * it waiting in Activity. */
export function JobQuestionHost() {
  const jobs = useJobs().data;
  const { followed, unfollow } = useFollowing();
  const action = useJobActions();
  // Hidden from the answer until the job's next snapshot replaces it.
  const [answered, setAnswered] = useState<string | null>(null);
  useEffect(() => {
    for (const id of followed) {
      const job = jobs?.find((each) => each.id === id);
      if (
        job &&
        (job.status === "completed" || job.status === "failed" || job.status === "cancelled")
      )
        unfollow(id);
    }
  }, [jobs, followed, unfollow]);
  const job = followed
    .map((id) => jobs?.find((each) => each.id === id))
    .find(
      (job) =>
        job?.status === "needs_user" && job.question && `${job.id}:${job.question.id}` !== answered,
    );
  const question = job?.question;
  if (!job || !question) return null;
  return (
    <QuestionDialog
      key={`${job.id}:${question.id}`}
      question={question}
      busy={action.isPending}
      error={action.isError ? action.error.message : undefined}
      close={() => {
        action.reset();
        unfollow(job.id);
      }}
      answer={(choiceId) =>
        action.mutate(
          { type: "answer", jobId: job.id, questionId: question.id, choiceId },
          { onSuccess: () => setAnswered(`${job.id}:${question.id}`) },
        )
      }
    />
  );
}
