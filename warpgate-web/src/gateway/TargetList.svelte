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
import AsyncButton from 'common/AsyncButton.svelte'
import GettingStarted from 'common/GettingStarted.svelte'
import EmptyState from 'common/EmptyState.svelte'
import GroupColorCircle from 'common/GroupColorCircle.svelte'
import IssuedPublicKeyModal from 'admin/IssuedPublicKeyModal.svelte'
import CreateOtpModal from 'admin/CreateOtpModal.svelte'
import Alert from 'common/sveltestrap-s5-ports/Alert.svelte'
import ModalHeader from 'common/sveltestrap-s5-ports/ModalHeader.svelte'
import ConfirmModal from 'common/ConfirmModal.svelte'

let instructionsTarget: TargetSnapshot|undefined = $state()
let credentialState: SelfServiceCredentialsState | undefined = $state()
let credentialStateLoaded = $state(false)
let credentialStateLoading = $state(false)
let credentialActionError: string | undefined = $state()
let issuingKeyTarget: TargetSnapshot | undefined = $state()
let issuingKeyModalOpen = $state(false)
let creatingOtpTarget: TargetSnapshot | undefined = $state()
let creatingOtpModalOpen = $state(false)
let revokingIssuedKeyTarget: TargetSnapshot | undefined = $state()
let revokingIssuedKeyModalOpen = $state(false)
let selectedIssuedPublicKeyId = $state('')
let removingOtpTarget: TargetSnapshot | undefined = $state()
let removeOtpModalOpen = $state(false)
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

function closeRevokeIssuedKeyModal () {
    revokingIssuedKeyModalOpen = false
    revokingIssuedKeyTarget = undefined
    selectedIssuedPublicKeyId = ''
}

function closeRemoveOtpModal () {
    removeOtpModalOpen = false
    removingOtpTarget = undefined
}

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

function issuedPublicKeysForTarget (target: TargetSnapshot): SelfServicePublicKeyCredential[] {
    return (credentialState?.publicKeys ?? [])
        .filter(credential =>
            credential.targetId === target.id
            && credential.issuedByWarpgate
            && !credential.revokedAt
        )
        .sort((a, b) => {
            const dateComparison = (b.dateAdded?.getTime() ?? 0) - (a.dateAdded?.getTime() ?? 0)
            if (dateComparison !== 0) {
                return dateComparison
            }
            return a.label.localeCompare(b.label)
        })
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

async function showRevokeIssuedKeyModal (target: TargetSnapshot) {
    credentialActionError = undefined
    try {
        await loadCredentialState(true)
        const credentials = issuedPublicKeysForTarget(target)
        const firstCredential = credentials[0]
        if (!firstCredential) {
            credentialActionError = `No issued SSH key found for ${target.name}`
            return
        }
        revokingIssuedKeyTarget = target
        selectedIssuedPublicKeyId = firstCredential.id
        revokingIssuedKeyModalOpen = true
    } catch (error) {
        credentialActionError = await formatError(error, 'Failed to load issued SSH keys')
    }
}

function selectedIssuedPublicKeyForRevocation (): SelfServicePublicKeyCredential | undefined {
    if (!revokingIssuedKeyTarget) {
        return undefined
    }
    return issuedPublicKeysForTarget(revokingIssuedKeyTarget)
        .find(credential => credential.id === selectedIssuedPublicKeyId)
}

async function revokeSelectedIssuedKey () {
    credentialActionError = undefined
    const credential = selectedIssuedPublicKeyForRevocation()
    if (!credential) {
        credentialActionError = 'Select an issued SSH key to revoke'
        return
    }

    try {
        await revokeMyPublicKeyCredential(credential.id)
        if (credentialState) {
            const revokedAt = new Date()
            credentialState.publicKeys = credentialState.publicKeys.map(existing => {
                if (existing.id !== credential.id) {
                    return existing
                }
                return {
                    ...existing,
                    revokedAt,
                    usesLeft: 0,
                }
            })
        }
        closeRevokeIssuedKeyModal()
    } catch (error) {
        credentialActionError = await formatError(error, 'Failed to revoke SSH key')
        throw error
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
        await deleteMyOtpCredential(credential.id)
        if (credentialState) {
            credentialState.otp = credentialState.otp.filter(c => c.id !== credential.id)
        }
        closeRemoveOtpModal()
    } catch (error) {
        credentialActionError = await formatError(error, 'Failed to remove OTP credential')
        throw error
    }
}

async function showRemoveOtpModal (target: TargetSnapshot) {
    credentialActionError = undefined
    try {
        await loadCredentialState()
        const credential = otpForTarget(target)
        if (!credential) {
            credentialActionError = `No target-scoped OTP credential found for ${target.name}`
            return
        }
        removingOtpTarget = target
        removeOtpModalOpen = true
    } catch (error) {
        credentialActionError = await formatError(error, 'Failed to load credentials')
    }
}

async function deleteSelectedOtpCredential () {
    if (!removingOtpTarget) {
        return
    }
    await deleteOtpForTarget(removingOtpTarget)
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

function abbreviatePublicKey (opensshPublicKey: string): string {
    const normalized = opensshPublicKey.trim()
    if (normalized.length <= 96) {
        return normalized
    }
    return `${normalized.slice(0, 64)}...${normalized.slice(-24)}`
}

function keySummary (credential: SelfServicePublicKeyCredential): string {
    return credential.abbreviated || abbreviatePublicKey(credential.opensshPublicKey)
}

function formatDate (date: Date | undefined, fallback: string): string {
    return date ? date.toLocaleString() : fallback
}

function formatUses (credential: SelfServicePublicKeyCredential): string {
    if (credential.maxUses === undefined) {
        return 'Unlimited'
    }
    return `${credential.usesLeft ?? 0} / ${credential.maxUses}`
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
                                disabled={credentialStateLoading || issuedPublicKeysForTarget(target).length === 0}
                                onclick={e => {
                                    e.preventDefault()
                                    e.stopPropagation()
                                    void showRevokeIssuedKeyModal(target)
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
                                    void showRemoveOtpModal(target)
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

<Modal isOpen={revokingIssuedKeyModalOpen} toggle={closeRevokeIssuedKeyModal} size="lg">
    <ModalHeader toggle={closeRevokeIssuedKeyModal}>
        Revoke issued SSH key
    </ModalHeader>
    <ModalBody>
        {#if revokingIssuedKeyTarget}
            {@const issuedKeys = issuedPublicKeysForTarget(revokingIssuedKeyTarget)}
            <div class="revoke-key-summary">
                <div class="summary-label">Target</div>
                <div class="summary-value">{revokingIssuedKeyTarget.name}</div>
            </div>

            {#if issuedKeys.length === 0}
                <p class="text-muted mb-0">No active issued SSH keys are available for this target.</p>
            {:else}
                <div class="issued-key-list" role="radiogroup" aria-label="Issued SSH keys">
                    {#each issuedKeys as credential (credential.id)}
                        <label
                            class="issued-key-option"
                            class:issued-key-option-selected={selectedIssuedPublicKeyId === credential.id}
                        >
                            <input
                                class="form-check-input issued-key-radio"
                                type="radio"
                                bind:group={selectedIssuedPublicKeyId}
                                value={credential.id}
                            />
                            <span class="issued-key-content">
                                <span class="issued-key-heading">
                                    <span class="issued-key-label">{credential.label || 'Issued SSH key'}</span>
                                    <span class="badge text-bg-info">Issued</span>
                                </span>
                                <code class="issued-key-public-key" title={credential.opensshPublicKey}>
                                    {keySummary(credential)}
                                </code>
                                <span class="issued-key-meta-grid">
                                    <span class="issued-key-meta">
                                        <span class="meta-label">Added</span>
                                        <span>{formatDate(credential.dateAdded, 'Unknown')}</span>
                                    </span>
                                    <span class="issued-key-meta">
                                        <span class="meta-label">Last used</span>
                                        <span>{formatDate(credential.lastUsed, 'Never')}</span>
                                    </span>
                                    <span class="issued-key-meta">
                                        <span class="meta-label">Expires</span>
                                        <span>{formatDate(credential.expiresAt, 'Never')}</span>
                                    </span>
                                    <span class="issued-key-meta">
                                        <span class="meta-label">Uses</span>
                                        <span>{formatUses(credential)}</span>
                                    </span>
                                </span>
                            </span>
                        </label>
                    {/each}
                </div>
            {/if}
        {/if}
    </ModalBody>
    <ModalFooter>
        <AsyncButton
            color="danger"
            class="modal-button"
            click={revokeSelectedIssuedKey}
            disabled={!selectedIssuedPublicKeyForRevocation()}
        >
            Revoke selected key
        </AsyncButton>
        <Button
            color="secondary"
            class="modal-button"
            onclick={closeRevokeIssuedKeyModal}
        >
            Cancel
        </Button>
    </ModalFooter>
</Modal>

<ConfirmModal
    bind:isOpen={removeOtpModalOpen}
    title="Remove OTP"
    message={`Remove the OTP credential for ${removingOtpTarget?.name ?? ''}?`}
    confirmLabel="Remove"
    onConfirm={deleteSelectedOtpCredential}
/>

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

    .revoke-key-summary {
        display: grid;
        grid-template-columns: 6rem 1fr;
        gap: .25rem 1rem;
        padding: .75rem 1rem;
        margin-bottom: 1rem;
        border: 1px solid var(--bs-border-color);
        border-radius: .5rem;
        background: var(--bs-tertiary-bg);
    }

    .summary-label,
    .meta-label {
        color: var(--bs-secondary-color);
        font-size: .75rem;
        font-weight: 600;
        letter-spacing: .02rem;
        text-transform: uppercase;
    }

    .summary-value {
        min-width: 0;
        overflow: hidden;
        font-weight: 600;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .issued-key-list {
        display: grid;
        gap: .75rem;
    }

    .issued-key-option {
        display: grid;
        grid-template-columns: auto minmax(0, 1fr);
        gap: .75rem;
        padding: 1rem;
        border: 1px solid var(--bs-border-color);
        border-radius: .5rem;
        background: var(--bs-body-bg);
        cursor: pointer;
        transition: border-color .15s ease, background-color .15s ease, box-shadow .15s ease;
    }

    .issued-key-option:hover {
        border-color: var(--bs-primary);
        background: var(--bs-tertiary-bg);
    }

    .issued-key-option-selected {
        border-color: var(--bs-primary);
        box-shadow: 0 0 0 .15rem color-mix(in srgb, var(--bs-primary) 18%, transparent);
    }

    .issued-key-radio {
        margin-top: .25rem;
    }

    .issued-key-content {
        display: grid;
        min-width: 0;
        gap: .5rem;
    }

    .issued-key-heading {
        display: flex;
        align-items: center;
        gap: .5rem;
        min-width: 0;
    }

    .issued-key-label {
        min-width: 0;
        overflow: hidden;
        font-weight: 600;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .issued-key-public-key {
        display: block;
        overflow: hidden;
        padding: .4rem .5rem;
        border-radius: .375rem;
        background: var(--bs-secondary-bg);
        color: var(--bs-body-color);
        font-size: .8125rem;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .issued-key-meta-grid {
        display: grid;
        grid-template-columns: repeat(2, minmax(0, 1fr));
        gap: .75rem 1rem;
    }

    .issued-key-meta {
        display: grid;
        min-width: 0;
        gap: .125rem;
        font-size: .875rem;
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

        .revoke-key-summary,
        .issued-key-meta-grid {
            grid-template-columns: 1fr;
        }
    }
</style>
