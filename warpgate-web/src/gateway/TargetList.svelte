<script lang="ts">
import { Observable, from, map } from 'rxjs'
import { compare as naturalCompareFactory } from 'natural-orderby'
import { faArrowRight, faEllipsisV, faTerminal } from '@fortawesome/free-solid-svg-icons'
import ConnectionInstructions from 'common/ConnectionInstructions.svelte'
import ItemList, { type LoadOptions, type PaginatedResponse } from 'common/ItemList.svelte'
import {
    api,
    type IssueMyPublicKeyArgs,
    type IssueMyPublicKeyResult,
    ResponseError,
    type SelfServiceCredentialsState,
    type SelfServiceOtpCredential,
    type SelfServicePublicKeyCredential,
    type TargetSnapshot,
    TargetKind,
    BootstrapThemeColor,
    createMyOtpCredential,
    deleteMyOtpCredential,
    getMyCredentialsForTargetActions,
    issueMyPublicKeyCredential,
    revokeMyPublicKeyCredential,
    stringifyError,
} from 'gateway/lib/api'
import Fa from 'svelte-fa'
import { Button, Dropdown, DropdownItem, DropdownMenu, DropdownToggle, Modal, ModalBody, ModalFooter } from '@sveltestrap/sveltestrap'
import { serverInfo } from './lib/store'
import { getContext } from 'svelte'
import { firstBy } from 'thenby'
import GettingStarted from 'common/GettingStarted.svelte'
import EmptyState from 'common/EmptyState.svelte'
import GroupColorCircle from 'common/GroupColorCircle.svelte'
import IssuedPublicKeyModal from 'admin/IssuedPublicKeyModal.svelte'
import CreateOtpModal from 'admin/CreateOtpModal.svelte'
import Alert from 'common/sveltestrap-s5-ports/Alert.svelte'

let instructionsTarget: TargetSnapshot|undefined = $state()
let credentialState: SelfServiceCredentialsState | undefined = $state()
let credentialStateLoaded = $state(false)
let credentialStateLoading = $state(false)
let credentialActionError: string | undefined = $state()
let issuingKeyTarget: TargetSnapshot | undefined = $state()
let issuingKeyModalOpen = $state(false)
let creatingOtpTarget: TargetSnapshot | undefined = $state()
let creatingOtpModalOpen = $state(false)
const getRoutePrefix = getContext<() => string>('warpgate.gatewayRoutePrefix') ?? (() => '')
const isEmbedded = getContext<() => boolean>('warpgate.gatewayEmbedded') ?? (() => false)

$effect(() => {
    if (!issuingKeyModalOpen) {
        issuingKeyTarget = undefined
    }
})

$effect(() => {
    if (!creatingOtpModalOpen) {
        creatingOtpTarget = undefined
    }
})

async function openWebSsh (target: TargetSnapshot) {
    const terminalWindow = window.open('', '_blank')

    try {
        const { sessionId } = await api.createWebSshSession({
            createWebSshSessionBody: { targetId: target.id },
        })
        const terminalUrl = `/@warpgate#/gateway/web-ssh/${sessionId}`
        if (terminalWindow) {
            terminalWindow.location.href = terminalUrl
            terminalWindow.focus()
        } else {
            location.href = terminalUrl
        }
    } catch (error) {
        terminalWindow?.close()
        credentialActionError = await formatError(error, 'Failed to open Web terminal')
    }
}

async function formatError (error: unknown, fallback: string): Promise<string> {
    if (error instanceof ResponseError) {
        return stringifyError(error)
    }
    return error instanceof Error ? error.message : fallback
}

async function loadCredentialState (force = false): Promise<SelfServiceCredentialsState | undefined> {
    if (!$serverInfo?.ownCredentialManagementAllowed) {
        return undefined
    }
    if (credentialStateLoaded && !force) {
        return credentialState
    }

    credentialStateLoading = true
    try {
        credentialState = await getMyCredentialsForTargetActions()
        credentialStateLoaded = true
        return credentialState
    } catch (error) {
        credentialActionError = await formatError(error, 'Failed to load credentials')
        throw error
    } finally {
        credentialStateLoading = false
    }
}

function issuedPublicKeyForTarget (target: TargetSnapshot): SelfServicePublicKeyCredential | undefined {
    return credentialState?.publicKeys.find(credential =>
        credential.targetId === target.id
        && credential.issuedByWarpgate
        && !credential.revokedAt
    )
}

function otpForTarget (target: TargetSnapshot): SelfServiceOtpCredential | undefined {
    return credentialState?.otp.find(credential => credential.targetId === target.id)
}

async function showIssueKeyModal (target: TargetSnapshot) {
    credentialActionError = undefined
    let state: SelfServiceCredentialsState | undefined
    try {
        state = await loadCredentialState()
    } catch {
        return
    }
    if (state?.ldapLinked) {
        credentialActionError = 'SSH keys are managed by LDAP for this account'
        return
    }
    issuingKeyTarget = target
    issuingKeyModalOpen = true
}

async function issueKeyForTarget (args: IssueMyPublicKeyArgs): Promise<IssueMyPublicKeyResult> {
    if (!issuingKeyTarget) {
        throw new Error('No SSH target selected')
    }

    const response = await issueMyPublicKeyCredential({
        ...args,
        targetId: issuingKeyTarget.id,
    })
    if (credentialState) {
        credentialState.publicKeys = [...credentialState.publicKeys, response.credential]
    }
    return response
}

async function revokeIssuedKeyForTarget (target: TargetSnapshot) {
    credentialActionError = undefined
    try {
        await loadCredentialState()
        const credential = issuedPublicKeyForTarget(target)
        if (!credential) {
            credentialActionError = `No issued SSH key found for ${target.name}`
            return
        }
        if (!confirm(`Revoke the issued SSH key for ${target.name}?`)) {
            return
        }
        await revokeMyPublicKeyCredential(credential.id)
        await loadCredentialState(true)
    } catch (error) {
        credentialActionError = await formatError(error, 'Failed to revoke SSH key')
    }
}

async function showCreateOtpModal (target: TargetSnapshot) {
    credentialActionError = undefined
    try {
        await loadCredentialState()
    } catch {
        return
    }
    creatingOtpTarget = target
    creatingOtpModalOpen = true
}

async function createOtpForTarget (secretKey: number[]) {
    if (!creatingOtpTarget) {
        throw new Error('No SSH target selected')
    }

    const credential = await createMyOtpCredential(secretKey, creatingOtpTarget.id)
    if (credentialState) {
        credentialState.otp = [
            ...credentialState.otp.filter(c => c.targetId !== creatingOtpTarget?.id),
            credential,
        ]
    }
}

async function deleteOtpForTarget (target: TargetSnapshot) {
    credentialActionError = undefined
    try {
        await loadCredentialState()
        const credential = otpForTarget(target)
        if (!credential) {
            credentialActionError = `No target-scoped OTP credential found for ${target.name}`
            return
        }
        if (!confirm(`Remove the OTP credential for ${target.name}?`)) {
            return
        }
        await deleteMyOtpCredential(credential.id)
        if (credentialState) {
            credentialState.otp = credentialState.otp.filter(c => c.id !== credential.id)
        }
    } catch (error) {
        credentialActionError = await formatError(error, 'Failed to remove OTP credential')
    }
}

function loadTargets(
    options: LoadOptions
): Observable<PaginatedResponse<TargetSnapshot>> {
    return from(api.getTargets({ search: options.search })).pipe(
        map(result => {
            const naturalCompare = naturalCompareFactory()

            result = result.sort(
                firstBy<TargetSnapshot, boolean>((x: TargetSnapshot) => !x.group)
                    // Natural sort between groups
                    .thenBy((a: TargetSnapshot, b: TargetSnapshot) =>
                        naturalCompare(
                            (a.group?.name ?? '').toLowerCase(),
                            (b.group?.name ?? '').toLowerCase()
                        )
                    )
                    // Natural sort within a group
                    .thenBy((a: TargetSnapshot, b: TargetSnapshot) =>
                        naturalCompare(
                            a.name.toLowerCase(),
                            b.name.toLowerCase()
                        )
                    )
            )

            return {
                items: result,
                offset: 0,
                total: result.length,
            }
        }),
    )
}

function selectTarget (target: TargetSnapshot) {
    if (target.kind === TargetKind.Http) {
        if (target.externalHost) {
            const port = location.port ? `:${location.port}` : ''
            loadURL(`${location.protocol}//${target.externalHost}${port}`)
        } else {
            loadURL(`/?warpgate-target=${target.name}`)
        }
    } else if (target.kind === TargetKind.Ssh) {
        void openWebSsh(target)
    } else {
        instructionsTarget = target
    }
}

function showInstructions (target: TargetSnapshot) {
    instructionsTarget = target
}

function loadURL (url: string) {
    location.href = url
}

interface GroupInfo {
    id: string
    name: string
    color: BootstrapThemeColor
}

function groupInfoFromTarget (target: TargetSnapshot): GroupInfo {
    if (!target.group) {
        return {
            id: '$ungrouped',
            name: 'Ungrouped',
            color: BootstrapThemeColor.Secondary,
        }
    }
    return {
        id: target.group.id,
        name: target.group.name,
        color: target.group.color ?? BootstrapThemeColor.Secondary,
    }
}

</script>

{#if $serverInfo?.setupState}
    <GettingStarted
        setupState={$serverInfo?.setupState} />
{/if}

{#if isEmbedded()}
    <div class="page-summary-bar">
        <h1>gateway</h1>
    </div>
{/if}

{#if credentialActionError}
    <Alert color="danger" fade={false} toggle={() => credentialActionError = undefined}>
        {credentialActionError}
    </Alert>
{/if}

<ItemList load={loadTargets} showSearch={true} groupObject={groupInfoFromTarget} groupKey={group => group.id}>
    {#snippet empty()}
        <EmptyState
            title="You don't have access to any targets yet" />
    {/snippet}
    {#snippet groupHeader(group)}
        <div class="d-flex align-items-center gap-2 mb-2 mt-4">
            <GroupColorCircle color={group.color} />
            <div class="h5 mb-0">{group.name}</div>
        </div>
    {/snippet}
    {#snippet item(target)}
        <a
            class="list-group-item list-group-item-action target-item gap-3"
            href={
                target.kind === TargetKind.Http
                    ? (target.externalHost
                        ? `${location.protocol}//${target.externalHost}${location.port ? `:${location.port}` : ''}`
                        : `/?warpgate-target=${target.name}`)
                    : `/@warpgate#${getRoutePrefix() || '/gateway'}`
            }
            onclick={e => {
                if (e.metaKey || e.ctrlKey) {
                    return
                }
                e.preventDefault()
                selectTarget(target)
            }}
        >
            <span class="target-main">
                <span class="target-name">{target.name}</span>
                {#if target.description}
                    <small class="target-description text-muted">{target.description}</small>
                {/if}
            </span>
            <small class="protocol text-muted ms-auto">
                {#if target.kind === TargetKind.MySql}
                    MySQL
                {/if}
                {#if target.kind === TargetKind.Postgres}
                    PostgreSQL
                {/if}
                {#if target.kind === TargetKind.Kubernetes}
                    Kubernetes
                {/if}
                {#if target.kind === TargetKind.Ssh}
                    SSH
                {/if}
            </small>
            {#if target.kind === TargetKind.Ssh}
                <Dropdown>
                    <DropdownToggle color="link" size="sm" class="target-action" onclick={e => {
                        e.preventDefault()
                        e.stopPropagation()
                        void loadCredentialState().catch(() => undefined)
                    }}>
                        <Fa icon={faEllipsisV} fw />
                    </DropdownToggle>
                    <DropdownMenu end>
                        <DropdownItem onclick={e => {
                            e.preventDefault()
                            e.stopPropagation()
                            void openWebSsh(target)
                        }}>Web terminal</DropdownItem>
                        <DropdownItem onclick={e => {
                            e.preventDefault()
                            e.stopPropagation()
                            showInstructions(target)
                        }}>Connection instructions</DropdownItem>
                        {#if $serverInfo?.ownCredentialManagementAllowed}
                            <DropdownItem divider />
                            <DropdownItem
                                disabled={credentialStateLoading || credentialState?.ldapLinked}
                                onclick={e => {
                                    e.preventDefault()
                                    e.stopPropagation()
                                    void showIssueKeyModal(target)
                                }}
                            >
                                Issue SSH key
                            </DropdownItem>
                            <DropdownItem
                                disabled={credentialStateLoading || !issuedPublicKeyForTarget(target)}
                                onclick={e => {
                                    e.preventDefault()
                                    e.stopPropagation()
                                    void revokeIssuedKeyForTarget(target)
                                }}
                            >
                                Revoke issued SSH key
                            </DropdownItem>
                            <DropdownItem
                                disabled={credentialStateLoading}
                                onclick={e => {
                                    e.preventDefault()
                                    e.stopPropagation()
                                    void showCreateOtpModal(target)
                                }}
                            >
                                Configure OTP
                            </DropdownItem>
                            <DropdownItem
                                disabled={credentialStateLoading || !otpForTarget(target)}
                                onclick={e => {
                                    e.preventDefault()
                                    e.stopPropagation()
                                    void deleteOtpForTarget(target)
                                }}
                            >
                                Remove OTP
                            </DropdownItem>
                        {/if}
                    </DropdownMenu>
                </Dropdown>
            {:else if target.kind === TargetKind.Http}
                <Button color="link" size="sm" tabindex={-1} class="target-action">
                    <Fa icon={faArrowRight} fw />
                </Button>
            {:else}
                <Button disabled color="link" size="sm" tabindex={-1} class="target-action" style="visibility: hidden">
                    <Fa icon={faEllipsisV} fw />
                </Button>
            {/if}
        </a>
    {/snippet}
</ItemList>

{#if $serverInfo?.setupState && !$serverInfo.setupState.hasTargets}
    <EmptyState
        hint="Once you add targets and assign access, they will appear here"
        title="No other targets yet" />
{/if}

<Modal isOpen={!!instructionsTarget} toggle={() => instructionsTarget = undefined} size="lg">
    <ModalBody>
        {#if instructionsTarget}
        <ConnectionInstructions
            targetName={instructionsTarget.name}
            username={$serverInfo?.username}
            targetKind={instructionsTarget.kind ?? TargetKind.Ssh}
            targetDefaultDatabaseName={
                (instructionsTarget.kind === TargetKind.MySql || instructionsTarget.kind === TargetKind.Postgres)
                    ? instructionsTarget.defaultDatabaseName : undefined}
        />
        {/if}
    </ModalBody>
    <ModalFooter>
        {#if instructionsTarget?.kind === TargetKind.Ssh}
            <Button
                color="primary"
                class="d-flex align-items-center justify-content-center gap-2 modal-button"
                onclick={() => { void openWebSsh(instructionsTarget!) }}
            >
                <Fa icon={faTerminal} />
                Open Web Terminal
            </Button>
        {/if}
        <Button
            color="secondary"
            class="modal-button"
            block
            on:click={() => { instructionsTarget = undefined }}
        >
            Close
        </Button>
    </ModalFooter>
</Modal>

{#if issuingKeyTarget}
    <IssuedPublicKeyModal
        bind:isOpen={issuingKeyModalOpen}
        issue={issueKeyForTarget}
        sshTargets={[{ id: issuingKeyTarget.id, name: issuingKeyTarget.name }]}
        defaultTargetId={issuingKeyTarget.id}
        allowGlobalTargetScope={false}
        onClose={() => {
            issuingKeyTarget = undefined
            issuingKeyModalOpen = false
        }}
    />
{/if}

{#if creatingOtpTarget}
    <CreateOtpModal
        bind:isOpen={creatingOtpModalOpen}
        username={$serverInfo?.username ?? ''}
        create={createOtpForTarget}
        sshTargets={[{ id: creatingOtpTarget.id, name: creatingOtpTarget.name }]}
        defaultTargetId={creatingOtpTarget.id}
        allowGlobalTargetScope={false}
    />
{/if}

<style lang="scss">
    .target-item {
        display: flex;
        align-items: center;
        min-height: 3rem;
    }

    .target-main {
        display: block;
        flex: 1 1 auto;
        min-width: 0;
    }

    .target-name,
    .target-description {
        display: block;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .target-name {
        font-weight: 600;
    }

    .target-description {
        line-height: 1.3;
    }

    .protocol {
        flex: 0 0 5.5rem;
        font-size: .75rem;
        font-weight: 600;
        letter-spacing: .02rem;
        text-align: right;
        text-transform: uppercase;
    }

    :global(.target-action.btn) {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        flex: 0 0 2rem;
        width: 2rem;
        min-height: 2rem;
        padding: 0;
    }

    @media (max-width: 576px) {
        .target-item {
            align-items: flex-start;
            flex-wrap: wrap;
        }

        .protocol {
            flex-basis: auto;
            margin-left: 0 !important;
            text-align: left;
        }
    }
</style>
