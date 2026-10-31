import { create } from 'zustand'
import { commands, DownloadJob, events } from '@/lib/bindings'
import {
  detectTransitions,
  removeJob as removeJobFromList,
  upsertJob,
} from './logic'

type Transition = 'completed' | 'failed'

type State = {
  jobs: DownloadJob[]
  initialized: boolean
  initialize: (
    onTransition: (job: DownloadJob, transition: Transition) => void,
  ) => Promise<void>
  removeJob: (id: string) => void
}

export const useDownloadJobStore = create<State>((set, get) => ({
  jobs: [],
  initialized: false,
  initialize: async (onTransition) => {
    if (get().initialized) {
      return
    }

    set({ initialized: true })

    let listening = false

    try {
      // 先に購読してから一覧を取得し、取得中に届いた更新を取りこぼさない
      await events.downloadJobUpdated.listen((e) => {
        const job = e.payload
        const transition = detectTransitions(
          get().jobs.find((j) => j.id === job.id),
          job,
        )

        set((state) => ({ jobs: upsertJob(state.jobs, job) }))

        if (transition !== null) {
          onTransition(job, transition)
        }
      })

      listening = true

      const result = await commands.listDownloadJobs()

      if (result.status === 'error') {
        console.error('Failed to list download jobs:', result.error)
        return
      }

      // 一覧の取得中に届いたイベントの方が新しいので、既にある工作は上書きしない
      set((state) => {
        const known = new Set(state.jobs.map((j) => j.id))

        return {
          jobs: [...state.jobs, ...result.data.filter((j) => !known.has(j.id))],
        }
      })
    } catch (error) {
      console.error('Failed to initialize download jobs:', error)

      // 購読できていない場合だけ、次のマウントで再試行できるようにする
      if (!listening) {
        set({ initialized: false })
      }
    }
  },
  removeJob: (id) => {
    set((state) => ({ jobs: removeJobFromList(state.jobs, id) }))
  },
}))
