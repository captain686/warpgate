<script lang="ts">
    import { Button, Modal, ModalBody, ModalFooter, type Color } from '@sveltestrap/sveltestrap'
    import AsyncButton from './AsyncButton.svelte'
    import ModalHeader from './sveltestrap-s5-ports/ModalHeader.svelte'

    interface Props {
        isOpen: boolean
        title: string
        message: string
        confirmLabel?: string
        cancelLabel?: string
        confirmColor?: Color | 'link'
        onConfirm: () => Promise<void> | void
    }

    let {
        isOpen = $bindable(false),
        title,
        message,
        confirmLabel = 'Confirm',
        cancelLabel = 'Cancel',
        confirmColor = 'danger',
        onConfirm,
    }: Props = $props()

    function close () {
        isOpen = false
    }

    async function confirmAndClose () {
        await onConfirm()
        close()
    }
</script>

<Modal {isOpen} toggle={close}>
    <ModalHeader toggle={close}>
        {title}
    </ModalHeader>
    <ModalBody>
        <p class="mb-0">{message}</p>
    </ModalBody>
    <ModalFooter>
        <AsyncButton
            color={confirmColor}
            class="modal-button"
            click={confirmAndClose}
        >
            {confirmLabel}
        </AsyncButton>
        <Button
            color="secondary"
            class="modal-button"
            onclick={close}
        >
            {cancelLabel}
        </Button>
    </ModalFooter>
</Modal>
