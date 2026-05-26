<script lang="ts">
    import { Button, FormGroup, Input, Modal, ModalBody, ModalFooter } from '@sveltestrap/sveltestrap'
    import AsyncButton from 'common/AsyncButton.svelte'
    import CopyButton from 'common/CopyButton.svelte'
    import Alert from 'common/sveltestrap-s5-ports/Alert.svelte'

    type KeyAlgorithm = 'ed25519' | 'rsa_sha512'

    interface IssuePublicKeyArgs {
        label: string
        targetId?: string
        validForSeconds?: number
        maxUses?: number
        algorithm: KeyAlgorithm
    }

    interface IssuePublicKeyResult {
        credential: {
            label: string
        }
        privateKeyOpenssh: string
    }

    interface TargetOption {
        id: string
        name: string
    }

    interface Props {
        isOpen: boolean
        issue: (args: IssuePublicKeyArgs) => Promise<IssuePublicKeyResult>
        sshTargets?: TargetOption[]
        defaultTargetId?: string
        allowGlobalTargetScope?: boolean
        onClose?: () => void
    }

    let {
        isOpen = $bindable(false),
        issue,
        sshTargets = [],
        defaultTargetId = '',
        allowGlobalTargetScope = true,
        onClose,
    }: Props = $props()

    let label = $state('')
    let algorithm = $state<KeyAlgorithm>('ed25519')
    let validForSeconds = $state('')
    let maxUses = $state('')
    let targetId = $state('')
    let errorText: string | undefined = $state()
    let privateKeyOpenssh = $state('')
    let issuedCredential: { label: string } | undefined = $state()

    function close() {
        isOpen = false
        label = ''
        algorithm = 'ed25519'
        validForSeconds = ''
        maxUses = ''
        targetId = ''
        errorText = undefined
        privateKeyOpenssh = ''
        issuedCredential = undefined
        onClose?.()
    }

    $effect(() => {
        if (isOpen) {
            targetId = defaultTargetId ?? ''
        }
    })

    function normalizeText(value: string | number | null | undefined): string {
        return String(value ?? '').trim()
    }

    function parsePositiveInt(value: string | number | null | undefined, fieldName: string): number | undefined {
        const text = normalizeText(value)
        if (!text) {
            return undefined
        }
        const parsed = Number.parseInt(text, 10)
        if (!Number.isFinite(parsed) || parsed <= 0) {
            throw new Error(`${fieldName} must be a positive integer`)
        }
        return parsed
    }

    async function issueCredential() {
        errorText = undefined
        if (!normalizeText(label)) {
            errorText = 'Label is required'
            return
        }

        try {
            const result = await issue({
                label: normalizeText(label),
                targetId: normalizeText(targetId) || undefined,
                algorithm,
                validForSeconds: parsePositiveInt(validForSeconds, 'Validity'),
                maxUses: parsePositiveInt(maxUses, 'Max uses'),
            })
            issuedCredential = result.credential
            privateKeyOpenssh = result.privateKeyOpenssh
        } catch (error) {
            errorText = error instanceof Error ? error.message : 'Failed to issue SSH key'
            throw error
        }
    }

    function downloadPrivateKey() {
        if (!privateKeyOpenssh) {
            return
        }
        const fileLabel = normalizeText(label) || 'warpgate'
        const filename = `${fileLabel}-private-key`
        const blob = new Blob([privateKeyOpenssh], { type: 'text/plain' })
        const url = URL.createObjectURL(blob)
        const a = document.createElement('a')
        a.href = url
        a.download = filename
        document.body.appendChild(a)
        a.click()
        document.body.removeChild(a)
        URL.revokeObjectURL(url)
    }
</script>

<Modal {isOpen} toggle={close}>
    <ModalBody>
        {#if privateKeyOpenssh}
            <Alert color="warning" fade={false}>
                This private key is only shown once. Save it now.
            </Alert>

            <FormGroup floating label="Issued private key (OpenSSH)">
                <textarea
                    class="form-control"
                    style="height: 16rem; font-family: monospace;"
                    readonly
                    value={privateKeyOpenssh}
                ></textarea>
            </FormGroup>

            {#if issuedCredential}
                <small class="text-muted d-block">
                    Credential: {issuedCredential.label}
                </small>
            {/if}
        {:else}
            <FormGroup floating label="Label">
                <Input bind:value={label} />
            </FormGroup>

            <FormGroup floating label="Algorithm">
                <select bind:value={algorithm} class="form-control">
                    <option value="ed25519">ED25519</option>
                    <option value="rsa_sha512">RSA (SHA-512)</option>
                </select>
            </FormGroup>

            <FormGroup floating label="Scope target (optional)">
                <select bind:value={targetId} class="form-control">
                    {#if allowGlobalTargetScope}
                        <option value="">All SSH targets</option>
                    {/if}
                    {#each sshTargets as target (target.id)}
                        <option value={target.id}>{target.name}</option>
                    {/each}
                </select>
            </FormGroup>

            <FormGroup floating label="Validity (seconds, optional)">
                <Input type="number" min="1" step="1" bind:value={validForSeconds} />
            </FormGroup>

            <FormGroup floating label="Max uses (optional)">
                <Input type="number" min="1" step="1" bind:value={maxUses} />
            </FormGroup>

            {#if errorText}
                <Alert color="danger" fade={false}>{errorText}</Alert>
            {/if}
        {/if}
    </ModalBody>

    <ModalFooter>
        {#if !privateKeyOpenssh}
            <AsyncButton
                color="primary"
                click={issueCredential}
                disabled={!normalizeText(label)}
            >
                Issue key
            </AsyncButton>
        {:else}
            <Button color="primary" on:click={downloadPrivateKey}>
                Save private key
            </Button>
            <CopyButton
                color="secondary"
                class="d-flex align-items-center justify-content-center"
                text={privateKeyOpenssh}
                label="Copy private key"
            />
        {/if}

        <Button color="danger" on:click={close}>
            Close
        </Button>
    </ModalFooter>
</Modal>
