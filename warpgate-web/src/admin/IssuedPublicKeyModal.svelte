<script lang="ts">
    import { Button, FormGroup, Input, Modal, ModalBody, ModalFooter } from '@sveltestrap/sveltestrap'
    import AsyncButton from 'common/AsyncButton.svelte'
    import CopyButton from 'common/CopyButton.svelte'
    import Alert from 'common/sveltestrap-s5-ports/Alert.svelte'
    import type { ExistingPublicKeyCredential } from './lib/api'

    type KeyAlgorithm = 'ed25519' | 'rsa_sha512'

    interface IssuePublicKeyArgs {
        label: string
        validForSeconds?: number
        maxUses?: number
        algorithm: KeyAlgorithm
    }

    interface IssuePublicKeyResult {
        credential: ExistingPublicKeyCredential
        privateKeyOpenssh: string
    }

    interface Props {
        isOpen: boolean
        issue: (args: IssuePublicKeyArgs) => Promise<IssuePublicKeyResult>
        onClose?: () => void
    }

    let {
        isOpen = $bindable(false),
        issue,
        onClose,
    }: Props = $props()

    let label = $state('')
    let algorithm = $state<KeyAlgorithm>('ed25519')
    let validForSeconds = $state('')
    let maxUses = $state('')
    let errorText: string | undefined = $state()
    let privateKeyOpenssh = $state('')
    let issuedCredential: ExistingPublicKeyCredential | undefined = $state()

    function close() {
        isOpen = false
        label = ''
        algorithm = 'ed25519'
        validForSeconds = ''
        maxUses = ''
        errorText = undefined
        privateKeyOpenssh = ''
        issuedCredential = undefined
        onClose?.()
    }

    function parsePositiveInt(value: string, fieldName: string): number | undefined {
        if (!value.trim()) {
            return undefined
        }
        const parsed = Number.parseInt(value, 10)
        if (!Number.isFinite(parsed) || parsed <= 0) {
            throw new Error(`${fieldName} must be a positive integer`)
        }
        return parsed
    }

    async function issueCredential() {
        errorText = undefined
        if (!label.trim()) {
            errorText = 'Label is required'
            return
        }

        try {
            const result = await issue({
                label: label.trim(),
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
        const fileLabel = label.trim() || 'warpgate'
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
                disabled={!label.trim()}
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
