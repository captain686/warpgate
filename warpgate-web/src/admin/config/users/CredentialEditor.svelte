<script lang="ts" module>
    import type {
        ExistingCertificateCredential as ExistingCertificateCredentialModel,
        ExistingOtpCredential as ExistingOtpCredentialModel,
        ExistingPasswordCredential as ExistingPasswordCredentialModel,
        ExistingPublicKeyCredential as ExistingPublicKeyCredentialModel,
        ExistingSsoCredential as ExistingSsoCredentialModel,
    } from '../../lib/api'

    export type ExtendedPublicKeyCredential = ExistingPublicKeyCredentialModel & {
        issuedByWarpgate?: boolean
        expiresAt?: Date
        maxUses?: number
        usesLeft?: number
        revokedAt?: Date
    }

    export type ExistingCredential =
        { kind: 'Password' } & ExistingPasswordCredentialModel
        | { kind: 'Sso' } & ExistingSsoCredentialModel
        | { kind: 'PublicKey' } & ExtendedPublicKeyCredential
        | { kind: 'Certificate' } & ExistingCertificateCredentialModel
        | { kind: 'Totp' } & ExistingOtpCredentialModel
</script>

<script lang="ts">
    import { faCertificate, faIdBadge, faKey, faKeyboard, faMobileScreen } from '@fortawesome/free-solid-svg-icons'
    import { api, CredentialKind, type ExistingPublicKeyCredential, type ExistingSsoCredential, type UserRequireCredentialsPolicy, type ParameterValues } from 'admin/lib/api'
    import { SvelteSet } from 'svelte/reactivity'
    import Fa from 'svelte-fa'
    import { Button } from '@sveltestrap/sveltestrap'
    import CreatePasswordModal from '../../CreatePasswordModal.svelte'
    import SsoCredentialModal from '../../SsoCredentialModal.svelte'
    import PublicKeyCredentialModal from '../../PublicKeyCredentialModal.svelte'
    import IssuedPublicKeyModal from '../../IssuedPublicKeyModal.svelte'
    import CertificateCredentialModal from '../../CertificateCredentialModal.svelte'
    import CreateOtpModal from '../../CreateOtpModal.svelte'
    import AuthPolicyEditor from './AuthPolicyEditor.svelte'
    import { abbreviatePublicKey, possibleCredentials } from 'common/protocols'
    import CredentialUsedStateBadge from 'common/CredentialUsedStateBadge.svelte'
    import Loadable from 'common/Loadable.svelte'
    import EmptyState from 'common/EmptyState.svelte'
    import Tooltip from 'common/sveltestrap-s5-ports/Tooltip.svelte'
    import { adminPermissions } from 'admin/lib/store'

    interface Props {
        userId: string
        username: string
        credentialPolicy: UserRequireCredentialsPolicy,
        ldapLinked?: boolean
    }
    let { userId, username, credentialPolicy = $bindable(), ldapLinked = false }: Props = $props()

    let credentials: ExistingCredential[] = $state([])
    let globalParameters: ParameterValues | undefined = $state()

    let creatingPassword = $state(false)
    let creatingOtp = $state(false)
    let editingSsoCredential = $state(false)
    let editingSsoCredentialInstance: ExistingSsoCredential|null = $state(null)
    let editingPublicKeyCredential = $state(false)
    let editingPublicKeyCredentialInstance: ExtendedPublicKeyCredential | null = $state(null)
    let editingCertificateCredential = $state(false)
    let issuingPublicKey = $state(false)

    const loadPromise = load()

    const policyProtocols: { id: 'ssh' | 'http' | 'mysql' | 'postgres' | 'kubernetes', name: string }[] = [
        { id: 'ssh', name: 'SSH' },
        { id: 'http', name: 'HTTP' },
        { id: 'mysql', name: 'MySQL' },
        { id: 'postgres', name: 'PostgreSQL' },
        { id: 'kubernetes', name: 'Kubernetes' },
    ]

    // Get effective possible credentials for a protocol, considering global SSH auth settings
    function getEffectivePossibleCredentials(protocolId: string): SvelteSet<CredentialKind> {
        const base = possibleCredentials[protocolId]
        if (!base) {
            return new SvelteSet()
        }

        // For SSH, filter based on global auth method settings
        if (protocolId === 'ssh' && globalParameters) {
            const filtered = new SvelteSet<CredentialKind>()
            for (const kind of base) {
                // PublicKey requires publickey auth enabled
                if (kind === CredentialKind.PublicKey && !globalParameters.sshClientAuthPublickey) {
                    continue
                }
                // Password requires password auth enabled
                if (kind === CredentialKind.Password && !globalParameters.sshClientAuthPassword) {
                    continue
                }
                // WebUserApproval requires keyboard-interactive.
                if (kind === CredentialKind.WebUserApproval && !globalParameters.sshClientAuthKeyboardInteractive) {
                    continue
                }
                // OTP requires both keyboard-interactive transport and OTP auth toggle.
                if (kind === CredentialKind.Totp && (!globalParameters.sshClientAuthKeyboardInteractive || !globalParameters.sshClientAuthOtp)) {
                    continue
                }
                filtered.add(kind)
            }
            return filtered
        }

        return new SvelteSet(base)
    }

    async function load () {
        await Promise.all([
            loadPasswords(),
            loadSso(),
            loadPublicKeys(),
            loadCertificates(),
            loadOtp(),
            loadParameters(),
        ])
    }

    async function loadParameters () {
        globalParameters = await api.getParameters({})
    }

    async function loadPasswords () {
        credentials.push(...(await api.getPasswordCredentials({ userId })).map(c => ({
            kind: CredentialKind.Password,
            ...c,
        })))
    }

    async function loadSso () {
        credentials.push(...(await api.getSsoCredentials({ userId })).map(c => ({
            kind: CredentialKind.Sso,
            ...c,
        })))
    }

    async function loadPublicKeys () {
        credentials.push(...(await api.getPublicKeyCredentials({ userId })).map(c => ({
            kind: CredentialKind.PublicKey,
            ...normalizePublicKeyCredential(c),
        })))
    }

    async function loadCertificates () {
        credentials.push(...(await api.getCertificateCredentials({ userId })).map(c => ({
            kind: CredentialKind.Certificate,
            ...c,
        })))
    }

    async function loadOtp () {
        credentials.push(...(await api.getOtpCredentials({ userId })).map(c => ({
            kind: CredentialKind.Totp,
            ...c,
        })))
    }

    async function deleteCredential (credential: ExistingCredential) {
        if (credential.kind === CredentialKind.Certificate) {
            if (!confirm('Permanently revoke certificate? This cannot be undone.')) {
                return
            }
        }

        if (credential.kind === CredentialKind.Password) {
            await api.deletePasswordCredential({
                id: credential.id,
                userId,
            })
        }
        if (credential.kind === CredentialKind.Sso) {
            await api.deleteSsoCredential({
                id: credential.id,
                userId,
            })
        }
        if (credential.kind === CredentialKind.PublicKey) {
            if (credential.issuedByWarpgate) {
                await revokeIssuedPublicKeyCredential(credential)
                return
            } else {
                await api.deletePublicKeyCredential({
                    id: credential.id,
                    userId,
                })
            }
        }
        if (credential.kind === CredentialKind.Certificate) {
            await api.revokeCertificateCredential({
                id: credential.id,
                userId,
            })
        }
        if (credential.kind === CredentialKind.Totp) {
            await api.deleteOtpCredential({
                id: credential.id,
                userId,
            })
        }

        credentials = credentials.filter(c => c !== credential)
    }

    async function createPassword (password: string) {
        const credential = await api.createPasswordCredential({
            userId,
            newPasswordCredential: {
                password,
            },
        })
        credentials.push({
            kind: CredentialKind.Password,
            ...credential,
        })
    }

    async function createOtp (secretKey: number[]) {
        const credential = await api.createOtpCredential({
            userId,
            newOtpCredential: {
                secretKey,
            },
        })
        credentials.push({
            kind: CredentialKind.Totp,
            ...credential,
        })

        // Automatically set up a 2FA policy when adding an OTP
        for (const protocol of ['http', 'ssh'] as ('http'|'ssh')[]) {
            for (const ck of [CredentialKind.Password, CredentialKind.PublicKey]) {
                const effectiveCreds = getEffectivePossibleCredentials(protocol)
                if (
                    !credentialPolicy[protocol]
                    && credentials.some(x => x.kind === ck)
                    && effectiveCreds.has(ck)
                ) {
                    credentialPolicy = {
                        ...credentialPolicy ?? {},
                        [protocol]: [ck, CredentialKind.Totp],
                    }
                }
            }
        }
    }

    async function saveSsoCredential (provider: string|null, email: string) {
        if (editingSsoCredentialInstance) {
            editingSsoCredentialInstance.provider = provider ?? undefined
            editingSsoCredentialInstance.email = email
            await api.updateSsoCredential({
                userId,
                id: editingSsoCredentialInstance.id,
                newSsoCredential: editingSsoCredentialInstance,
            })
        } else {
            const credential = await api.createSsoCredential({
                userId,
                newSsoCredential: {
                    provider:provider ?? undefined,
                    email,
                },
            })
            credentials.push({
                kind: CredentialKind.Sso,
                ...credential,
            })
        }
        editingSsoCredential = false
        editingSsoCredentialInstance = null
    }

    async function savePublicKeyCredential (label: string, opensshPublicKey: string) {
        if (editingPublicKeyCredentialInstance) {
            editingPublicKeyCredentialInstance.label = label
            editingPublicKeyCredentialInstance.opensshPublicKey = opensshPublicKey
            await api.updatePublicKeyCredential({
                userId,
                id: editingPublicKeyCredentialInstance.id,
                newPublicKeyCredential: editingPublicKeyCredentialInstance,
            })
        } else {
            const credential = await api.createPublicKeyCredential({
                userId,
                newPublicKeyCredential: {
                    label,
                    opensshPublicKey,
                },
            })
            credentials.push({
                kind: CredentialKind.PublicKey,
                ...credential,
            })
        }
        editingPublicKeyCredential = false
        editingPublicKeyCredentialInstance = null
    }

    function parseMaybeDate(value: unknown): Date | undefined {
        if (!value) {
            return undefined
        }
        if (value instanceof Date) {
            return value
        }
        const parsed = new Date(String(value))
        return Number.isNaN(parsed.getTime()) ? undefined : parsed
    }

    function parseMaybeNumber(value: unknown): number | undefined {
        if (value === null || value === undefined || value === '') {
            return undefined
        }
        const parsed = Number(value)
        return Number.isFinite(parsed) ? parsed : undefined
    }

    function normalizePublicKeyCredential(
        raw: ExistingPublicKeyCredential | Record<string, unknown>
    ): ExtendedPublicKeyCredential {
        const rawRecord = raw as Record<string, unknown>
        const id = String(rawRecord.id ?? '')
        const label = String(rawRecord.label ?? '')
        const opensshPublicKey = String(rawRecord.opensshPublicKey ?? rawRecord.openssh_public_key ?? '')

        return {
            ...(rawRecord as unknown as ExistingPublicKeyCredential),
            id,
            label,
            opensshPublicKey,
            dateAdded: parseMaybeDate(rawRecord.dateAdded ?? rawRecord.date_added),
            lastUsed: parseMaybeDate(rawRecord.lastUsed ?? rawRecord.last_used),
            issuedByWarpgate: Boolean(rawRecord.issuedByWarpgate ?? rawRecord.issued_by_warpgate),
            expiresAt: parseMaybeDate(rawRecord.expiresAt ?? rawRecord.expires_at),
            maxUses: parseMaybeNumber(rawRecord.maxUses ?? rawRecord.max_uses),
            usesLeft: parseMaybeNumber(rawRecord.usesLeft ?? rawRecord.uses_left),
            revokedAt: parseMaybeDate(rawRecord.revokedAt ?? rawRecord.revoked_at),
        }
    }

    async function issuePublicKeyCredential(args: {
        label: string
        validForSeconds?: number
        maxUses?: number
        algorithm: 'ed25519' | 'rsa_sha512'
    }): Promise<{ credential: ExtendedPublicKeyCredential, privateKeyOpenssh: string }> {
        const body = await api.issuePublicKeyCredential({
            userId,
            issuePublicKeyCredentialRequest: {
                label: args.label,
                validForSeconds: args.validForSeconds,
                maxUses: args.maxUses,
                algorithm: args.algorithm,
            },
        })
        const credential = normalizePublicKeyCredential(
            (body.credential ?? {}) as ExistingPublicKeyCredential | Record<string, unknown>
        )

        credentials.push({
            kind: CredentialKind.PublicKey,
            ...credential,
        })

        return {
            credential,
            privateKeyOpenssh: String(body.privateKeyOpenssh ?? ''),
        }
    }

    async function revokeIssuedPublicKeyCredential(credential: ExtendedPublicKeyCredential) {
        if (!confirm('Revoke this issued SSH key? This cannot be undone.')) {
            return
        }

        await api.revokePublicKeyCredential({
            userId,
            id: credential.id,
        })

        credentials = credentials.map(c => {
            if (c.kind !== CredentialKind.PublicKey || c.id !== credential.id) {
                return c
            }
            return {
                ...c,
                revokedAt: new Date(),
                usesLeft: 0,
            }
        })
    }

    async function saveCertificateCredential (label: string, publicKeyPem: string) {
        const response = await api.issueCertificateCredential({
            userId,
            issueCertificateCredentialRequest: {
                label,
                publicKeyPem,
            },
        })

        credentials.push({
            kind: CredentialKind.Certificate,
            ...response.credential,
        })

        return response
    }
</script>

<div class="d-flex mt-4 mb-2 header">
    <h4 class="m-0">Credentials</h4>
    <span class="ms-auto"></span>
    {#if $adminPermissions.usersEdit}
    <Button size="sm" color="link" on:click={() => creatingPassword = true}>
        Add password
    </Button>
    <Button size="sm" color="link" on:click={() => {
        editingCertificateCredential = true
    }}>Issue certificate</Button>
    <Button
        id="addPublicKeyCredentialButton"
        size="sm"
        color="link"
        on:click={() => {
            if (ldapLinked) {
                return
            }
            editingPublicKeyCredentialInstance = null
            editingPublicKeyCredential = true
        }}
        title={ldapLinked ? 'SSH keys are managed by LDAP' : ''}
    >Add public key</Button>
    <Tooltip delay="250" target="addPublicKeyCredentialButton" animation>Public key credentials will be loaded from LDAP</Tooltip>
    <Button
        size="sm"
        color="link"
        disabled={ldapLinked}
        title={ldapLinked ? 'SSH keys are managed by LDAP' : ''}
        on:click={() => {
            if (ldapLinked) {
                return
            }
            issuingPublicKey = true
        }}
    >Issue SSH key</Button>

    <Button size="sm" color="link" on:click={() => creatingOtp = true}>Add OTP</Button>
    <Button size="sm" color="link" on:click={() => {
        editingSsoCredentialInstance = null
        editingSsoCredential = true
    }}>Add SSO</Button>
    {/if}
</div>

<Loadable promise={loadPromise}>
    {#if credentials.length === 0}
        <EmptyState
            title="No credentials added"
            hint="Users need credentials to authenticate with Warpgate"
        />
    {/if}
    <div class="list-group list-group-flush mb-3">
        {#each credentials as credential (credential.id)}
        <div class="list-group-item credential gap-2">
            {#if credential.kind === CredentialKind.Password }
                <Fa fw icon={faKeyboard} />
                <span class="label me-auto">Password</span>
            {/if}
            {#if credential.kind === 'PublicKey'}
                <Fa fw icon={faKey} />
                <div class="main me-auto">
                    <div class="label d-flex align-items-center">
                        {credential.label}
                        {#if credential.issuedByWarpgate}
                            <span class="badge text-bg-info ms-2">Issued</span>
                        {/if}
                        {#if credential.revokedAt}
                            <span class="badge text-bg-danger ms-2">Revoked</span>
                        {/if}
                    </div>
                    <small class="d-block text-muted">{abbreviatePublicKey(credential.opensshPublicKey)}</small>
                    {#if credential.issuedByWarpgate}
                        <small class="d-block text-muted">
                            {#if credential.expiresAt}
                                Expires: {credential.expiresAt.toLocaleString()}
                            {:else}
                                Expires: never
                            {/if}
                            ·
                            {#if credential.maxUses !== undefined}
                                Uses left: {credential.usesLeft ?? 0} / {credential.maxUses}
                            {:else}
                                Uses: unlimited
                            {/if}
                        </small>
                    {/if}
                </div>
                <CredentialUsedStateBadge credential={credential} />
            {/if}
            {#if credential.kind === CredentialKind.Certificate}
                <Fa fw icon={faCertificate} />
                <div class="main me-auto abbreviate">
                    <div class="label d-flex align-items-center">
                        {credential.label}
                    </div>
                    <small class="d-block text-muted abbreviate">SHA-256: <code>{credential.fingerprint}</code></small>
                </div>
                <CredentialUsedStateBadge credential={credential} />
            {/if}
            {#if credential.kind === 'Totp'}
                <Fa fw icon={faMobileScreen} />
                <span class="label me-auto">One-time password</span>
            {/if}
            {#if credential.kind === CredentialKind.Sso}
                <Fa fw icon={faIdBadge} />
                <span class="label">Single sign-on</span>
                <span class="text-muted me-auto">
                    {credential.email}
                    {#if credential.provider} ({credential.provider}){/if}
                </span>
            {/if}

            {#if credential.kind === CredentialKind.PublicKey || credential.kind === CredentialKind.Sso}
            <Button
                class="px-0"
                color="link"
                disabled={credential.kind === CredentialKind.PublicKey && (ldapLinked || !$adminPermissions.usersEdit || credential.issuedByWarpgate)}
                onclick={e => {
                    if (credential.kind === CredentialKind.Sso) {
                        editingSsoCredentialInstance = credential
                        editingSsoCredential = true
                    }
                    if (credential.kind === CredentialKind.PublicKey) {
                        editingPublicKeyCredentialInstance = credential
                        editingPublicKeyCredential = true
                    }
                    e.preventDefault()
                }}>
                Change
            </Button>
            {/if}
            {#if credential.kind === CredentialKind.PublicKey && credential.issuedByWarpgate && !credential.revokedAt}
            <Button
                class="px-0"
                color="link"
                disabled={ldapLinked || !$adminPermissions.usersEdit}
                onclick={e => {
                    revokeIssuedPublicKeyCredential(credential)
                    e.preventDefault()
                }}>
                Revoke
            </Button>
            {/if}
            {#if credential.kind !== CredentialKind.PublicKey || !credential.issuedByWarpgate}
            <Button
                class="px-0"
                color="link"
                disabled={credential.kind === CredentialKind.PublicKey && (ldapLinked || !$adminPermissions.usersEdit)}
                onclick={e => {
                    deleteCredential(credential)
                    e.preventDefault()
                }}>
                Delete
            </Button>
            {/if}
        </div>
        {/each}
    </div>

    <h4>Auth policy</h4>
    <div class="list-group list-group-flush mb-3">
        {#each policyProtocols as protocol (protocol)}
            {@const effectiveCredentials = getEffectivePossibleCredentials(protocol.id)}
            <div class="list-group-item">
                <div class="mb-1">
                    <strong>{protocol.name}</strong>
                </div>
                {#if effectiveCredentials.size > 0}
                    <AuthPolicyEditor
                        bind:value={credentialPolicy}
                        existingCredentials={credentials}
                        possibleCredentials={effectiveCredentials}
                        protocolId={protocol.id}
                    />
                {:else}
                    <span class="text-muted">No authentication methods available for this protocol</span>
                {/if}
            </div>
        {/each}
    </div>
</Loadable>

{#if creatingPassword}
<CreatePasswordModal
    bind:isOpen={creatingPassword}
    create={createPassword}
/>
{/if}

{#if creatingOtp}
<CreateOtpModal
    bind:isOpen={creatingOtp}
    {username}
    create={createOtp}
/>
{/if}

{#if editingSsoCredential}
<SsoCredentialModal
    bind:isOpen={editingSsoCredential}
    instance={editingSsoCredentialInstance}
    save={saveSsoCredential}
/>
{/if}

{#if editingPublicKeyCredential}
<PublicKeyCredentialModal
    bind:isOpen={editingPublicKeyCredential}
    instance={editingPublicKeyCredentialInstance ?? undefined}
    save={savePublicKeyCredential}
/>
{/if}

{#if issuingPublicKey}
<IssuedPublicKeyModal
    bind:isOpen={issuingPublicKey}
    issue={issuePublicKeyCredential}
    onClose={() => {
        issuingPublicKey = false
    }}
/>
{/if}

{#if editingCertificateCredential}
<CertificateCredentialModal
    bind:isOpen={editingCertificateCredential}
    save={saveCertificateCredential}
    {username}
    onClose={() => {
        editingCertificateCredential = false
    }}
/>
{/if}

<style lang="scss">
    .credential {
        display: flex;
        align-items: center;

        .label:not(:first-child), .main {
            margin-left: .75rem;
        }
    }

    .header {
        align-items: center;
    }

    @media (max-width: 720px) {
        .header {
            flex-direction: column;
            align-items: start;
        }
    }
</style>
