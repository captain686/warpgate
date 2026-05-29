<script lang="ts">
    import { api, BootstrapThemeColor, type TargetGroup } from 'admin/lib/api'
    import { Button, FormGroup, Input, Label } from '@sveltestrap/sveltestrap'
    import { stringifyError } from 'common/errors'
    import { VALID_CHOICES } from './common'
    import GroupColorCircle from 'common/GroupColorCircle.svelte'
    import AsyncButton from 'common/AsyncButton.svelte'
    import Loadable from 'common/Loadable.svelte'
    import { replace } from 'svelte-spa-router'
    import Alert from 'common/sveltestrap-s5-ports/Alert.svelte'
    import { adminPermissions } from 'admin/lib/store'
    import ConfirmModal from 'common/ConfirmModal.svelte'

    interface Props {
        params: { id: string };
    }

    let { params }: Props = $props()
    let groupId = $derived(params.id)

    let group: TargetGroup | undefined = $state()
    let error: string | undefined = $state()
    let saving = $state(false)
    let deleteGroupModalOpen = $state(false)

    let name = $state('')
    let description = $state('')
    let color = $state<BootstrapThemeColor | ''>('')

    const initPromise = init()

    function canAccessTargetGroupConfig(): boolean {
        return $adminPermissions.targetsCreate
            || $adminPermissions.targetsEdit
            || $adminPermissions.targetsDelete
    }

    function canEditTargetGroup(): boolean {
        return $adminPermissions.targetsEdit
    }

    function canDeleteTargetGroup(): boolean {
        return $adminPermissions.targetsDelete
    }

    async function init () {
        if (!canAccessTargetGroupConfig()) {
            return
        }

        try {
            group = await api.getTargetGroup({ id: groupId })
            name = group.name
            description = group.description
            color = group.color ?? ''
        } catch (e) {
            error = await stringifyError(e)
            throw e
        }
    }

    async function update () {
        if (!canEditTargetGroup()) {
            return
        }

        if (!group) {
            return
        }

        saving = true
        error = undefined

        try {
            await api.updateTargetGroup({
                id: groupId,
                targetGroupDataRequest: {
                    name,
                    description: description || undefined,
                    color: color || undefined,
                },
            })
        } catch (e) {
            error = await stringifyError(e)
            throw e
        } finally {
            saving = false
        }
    }

    function requestRemove () {
        deleteGroupModalOpen = true
    }

    async function remove () {
        if (!canDeleteTargetGroup()) {
            return
        }

        if (!group) {
            return
        }

        try {
            await api.deleteTargetGroup({ id: groupId })
            // Redirect to groups list
            replace('/config/target-groups')
        } catch (e) {
            error = await stringifyError(e)
            throw e
        }
    }
</script>


{#if error}
    <Alert color="danger">{error}</Alert>
{/if}
{#if canAccessTargetGroupConfig()}
    <Loadable promise={initPromise}>
    {#if group}
        <div class="container-max-md">
            <div class="page-summary-bar">
                <div>
                    <h1>{group.name}</h1>
                    <div class="text-muted">Target group</div>
                </div>
            </div>

            {#if !canEditTargetGroup()}
                <Alert color="secondary" class="mb-3">
                    Target group configuration is view-only for your administrator role.
                </Alert>
            {/if}

            <form onsubmit={e => {
                e.preventDefault()
                update()
            }}>
                <fieldset class="target-group-fieldset" disabled={saving || !canEditTargetGroup()}>
                    <FormGroup>
                        <Label for="name">Name</Label>
                        <Input
                            id="name"
                            bind:value={name}
                            required
                            disabled={saving || !canEditTargetGroup()}
                        />
                    </FormGroup>

                    <FormGroup>
                        <Label for="description">Description</Label>
                        <Input
                            id="description"
                            type="textarea"
                            bind:value={description}
                            disabled={saving || !canEditTargetGroup()}
                        />
                    </FormGroup>

                    <FormGroup>
                        <Label for="color">Color</Label>
                        <small class="form-text text-muted">
                            Optional Bootstrap theme color for visual organization
                        </small>
                        <div class="color-picker">
                            {#each VALID_CHOICES as value (value)}
                                <button
                                    type="button"
                                    class="btn btn-secondary gap-2 d-flex align-items-center"
                                    class:active={color === value}
                                    disabled={saving || !canEditTargetGroup()}
                                    onclick={(e) => {
                                        e.preventDefault()
                                        color = value
                                    }}
                                    title={value || 'None'}
                                >
                                    <GroupColorCircle color={value} />
                                    <span>{value || 'None'}</span>
                                </button>
                            {/each}
                        </div>
                    </FormGroup>
                </fieldset>

                <div class="d-flex gap-2 mt-5">
                    <AsyncButton
                        click={update}
                        color="primary"
                        disabled={!canEditTargetGroup()}
                    >Update</AsyncButton>
                    <Button
                        color="danger"
                        onclick={requestRemove}
                        disabled={!canDeleteTargetGroup()}
                    >Remove</Button>
                </div>
            </form>
        </div>
    {/if}
    </Loadable>
{:else}
    <Alert color="warning">
        You have no permission to manage target groups.
    </Alert>
{/if}

<ConfirmModal
    bind:isOpen={deleteGroupModalOpen}
    title="Delete target group"
    message={`Delete target group "${group?.name ?? ''}"?`}
    confirmLabel="Delete"
    onConfirm={remove}
/>

<style lang="scss">
    .target-group-fieldset {
        border: 0;
        min-inline-size: auto;
        margin: 0;
        padding: 0;
    }

    .color-picker {
        display: flex;
        flex-wrap: wrap;
        gap: 0.5rem;
    }
</style>
