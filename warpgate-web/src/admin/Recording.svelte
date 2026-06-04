<script lang="ts">
    import { api, RecordingKind, type Recording } from 'admin/lib/api'
    import TerminalRecordingPlayer from 'admin/player/TerminalRecordingPlayer.svelte'
    import Alert from 'common/sveltestrap-s5-ports/Alert.svelte'
    import DelayedSpinner from 'common/DelayedSpinner.svelte'
    import { stringifyError } from 'common/errors'
    import KubernetesRecording from './KubernetesRecording.svelte'
    import Loadable from 'common/Loadable.svelte'

    interface Props {
        params: { id: string }
    }

    let { params = { id: '' } }: Props = $props()

    let error: string|null = $state(null)
    import { adminPermissions } from './lib/store'
    let recording: Recording|null = $state(null)

    async function load () {
        if (!$adminPermissions.recordingsView) {
            error = 'You do not have permission to view recordings.'
            return
        }
        recording = await api.getRecording(params)
    }

    function getTCPDumpURL () {
        return `/@warpgate/admin/api/recordings/${recording?.id}/tcpdump`
    }

    load().catch(async e => {
        error = await stringifyError(e)
    })
</script>

<div class="recording-page">
    <div class="page-summary-bar">
        <h1>session recording</h1>
    </div>

    {#if !recording && !error}
        <DelayedSpinner />
    {/if}

    {#if error}
    <Alert color="danger">{error}</Alert>
    {/if}

    {#if recording?.kind === RecordingKind.Traffic}
        <a href={getTCPDumpURL()}>Download tcpdump file</a>
    {/if}
    {#if recording?.kind === RecordingKind.Terminal}
        <div class="recording-player">
            <TerminalRecordingPlayer recording={recording} />
        </div>
    {/if}
    {#if recording?.kind === RecordingKind.Kubernetes}
        <Loadable promise={api.getKubernetesRecording({ id: recording.id })}>
            {#snippet children(items)}
                <KubernetesRecording items={items} />
            {/snippet}
        </Loadable>
    {/if}
</div>

<style lang="scss">
    .recording-page {
        box-sizing: border-box;
        display: flex;
        flex: 1 1 auto;
        flex-direction: column;
        gap: .75rem;
        width: 100%;
        height: 100%;
        min-height: 0;
        min-width: 0;
        max-width: 100%;
        max-height: 100%;
        overflow: hidden;
    }

    .recording-page :global(.page-summary-bar) {
        flex: none;
        margin-bottom: 0;
    }

    .recording-player {
        display: flex;
        flex: 1 1 auto;
        height: 0;
        min-height: 0;
        min-width: 0;
        max-width: 100%;
        max-height: 100%;
        overflow: hidden;
    }

    .recording-player :global(.root) {
        flex: 1 1 auto;
        min-height: 0;
        min-width: 0;
    }
</style>
