<script lang="ts">
import { Input } from '@sveltestrap/sveltestrap'
import { CredentialKind, type UserRequireCredentialsPolicy } from 'admin/lib/api'
import type { ExistingCredential } from './CredentialEditor.svelte'
import InfoBox from 'common/InfoBox.svelte'
import { SvelteSet } from 'svelte/reactivity'

type ProtocolID = 'http' | 'ssh' | 'mysql' | 'postgres' | 'kubernetes'

interface Props {
    value: UserRequireCredentialsPolicy
    possibleCredentials: Set<CredentialKind>
    existingCredentials: ExistingCredential[]
    protocolId: ProtocolID
}

let {
    value = $bindable(),
    possibleCredentials,
    existingCredentials,
    protocolId,
}: Props = $props()

const labels = {
    Password: 'Password',
    PublicKey: 'Key',
    Certificate: 'Certificate',
    Totp: 'OTP',
    Sso: 'SSO',
    WebUserApproval: 'In-browser auth',
}

const credentialOrder: CredentialKind[] = [
    CredentialKind.Password,
    CredentialKind.PublicKey,
    CredentialKind.Totp,
    CredentialKind.Sso,
    CredentialKind.WebUserApproval,
    CredentialKind.Certificate,
]

const tips: Record<ProtocolID, Map<[CredentialKind, boolean], string>> = {
    postgres: new Map([
        [
            [CredentialKind.WebUserApproval, true],
            'Not all clients will show the 2FA auth prompt. The user might need to log in to the Warpgate UI to see the prompt.',
        ],
    ]),
    http: new Map(),
    mysql: new Map(),
    ssh: new Map(),
    kubernetes: new Map([
        [
            [CredentialKind.WebUserApproval, true],
            'Users will need to log in to the Warpgate UI to see the 2FA auth prompt for Kubernetes access.',
        ],
    ]),
}

let activeTips: string[] = $derived.by(() => {
    let result = []
    for (const [[kind, enabled], tip] of tips[protocolId]?.entries() ?? []) {
        if (value[protocolId]?.includes(kind) === enabled) {
            result.push(tip)
        }
    }
    return result
})

const validCredentials = $derived.by(() => {
    const vc = new SvelteSet(existingCredentials.map(x => x.kind as CredentialKind))
    vc.add(CredentialKind.WebUserApproval)
    return vc
})

const displayCredentials = $derived(
    credentialOrder.filter(type => possibleCredentials.has(type))
)

let isAny = $derived(!value[protocolId])

function updateAny () {
    if (isAny) {
        value[protocolId] = undefined
    } else {
        value[protocolId] = []
        let oneCred = displayCredentials.find(x => validCredentials.has(x))
        if (oneCred) {
            value[protocolId] = [oneCred]
        }
    }
}

function toggle (type: CredentialKind) {
    if (!validCredentials.has(type)) {
        return
    }
    if (value[protocolId]!.includes(type)) {
        value[protocolId] = value[protocolId]!.filter((x: CredentialKind) => x !== type)
    } else {
        value[protocolId]!.push(type)
    }
}
</script>

<div class="d-flex wrapper">
    <Input
        id={'policy-editor-' + protocolId}
        type="switch"
        bind:checked={isAny}
        label="Any credential"
        on:change={updateAny}
    />
    {#if !isAny}
        {#each displayCredentials as type (type)}
            <Input
                id={'policy-editor-' + protocolId + type}
                type="switch"
                checked={value[protocolId]?.includes(type)}
                label={labels[type]}
                disabled={!validCredentials.has(type)}
                title={!validCredentials.has(type) ? 'Add this credential first' : ''}
                on:change={() => toggle(type)}
            />
        {/each}
    {/if}
</div>

{#each activeTips as tip (tip)}
    <InfoBox class="mt-2">{tip}</InfoBox>
{/each}

<style lang="scss">
    .wrapper {
        flex-wrap: wrap;
        :global(.form-switch) {
            margin-right: 1rem;
        }
    }
</style>
