import { useState, useCallback, useEffect } from 'react';

type ModalVariant = 'default' | 'confirmation' | 'alert' | 'loading';

interface ModalState {
  isOpen: boolean;
  data?: unknown;
  variant: ModalVariant;
}

interface UseModalProps {
  initialOpen?: boolean;
  closeOnEscape?: boolean;
  closeOnOverlayClick?: boolean;
  onOpen?: () => void;
  onClose?: () => void;
}

interface UseModalReturn {
  isOpen: boolean;
  data: unknown;
  variant: ModalVariant;
  open: (data?: unknown, variant?: ModalVariant) => void;
  close: () => void;
  toggle: () => void;
  setData: (data: unknown) => void;
  setVariant: (variant: ModalVariant) => void;
}

export function useModal({
  initialOpen = false,
  closeOnEscape = true,
  onOpen,
  onClose,
}: UseModalProps = {}): UseModalReturn {
  const [modalState, setModalState] = useState<ModalState>({
    isOpen: initialOpen,
    data: null,
    variant: 'default',
  });

  const open = useCallback((data?: unknown, variant: ModalVariant = 'default') => {
    setModalState({
      isOpen: true,
      data,
      variant,
    });
    onOpen?.();
  }, [onOpen]);

  const close = useCallback(() => {
    setModalState(prev => ({
      ...prev,
      isOpen: false,
    }));
    onClose?.();
  }, [onClose]);

  const toggle = useCallback(() => {
    if (modalState.isOpen) {
      close();
    } else {
      open();
    }
  }, [modalState.isOpen, open, close]);

  const setData = useCallback((data: unknown) => {
    setModalState(prev => ({
      ...prev,
      data,
    }));
  }, []);

  const setVariant = useCallback((variant: ModalVariant) => {
    setModalState(prev => ({
      ...prev,
      variant,
    }));
  }, []);

  // Handle escape key
  useEffect(() => {
    if (!closeOnEscape || !modalState.isOpen) return;

    const handleEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        close();
      }
    };

    document.addEventListener('keydown', handleEscape);
    return () => document.removeEventListener('keydown', handleEscape);
  }, [closeOnEscape, modalState.isOpen, close]);

  // Manage body scroll
  useEffect(() => {
    if (modalState.isOpen) {
      document.body.style.overflow = 'hidden';
      return () => {
        document.body.style.overflow = 'unset';
      };
    }
  }, [modalState.isOpen]);

  return {
    isOpen: modalState.isOpen,
    data: modalState.data,
    variant: modalState.variant,
    open,
    close,
    toggle,
    setData,
    setVariant,
  };
}

// Hook for managing multiple modals
interface UseModalStackReturn {
  modals: Map<string, ModalState>;
  openModal: (id: string, data?: unknown, variant?: ModalVariant) => void;
  closeModal: (id: string) => void;
  closeAllModals: () => void;
  isModalOpen: (id: string) => boolean;
  getModalData: (id: string) => unknown;
  hasOpenModals: boolean;
}

export function useModalStack(): UseModalStackReturn {
  const [modals, setModals] = useState<Map<string, ModalState>>(new Map());

  const openModal = useCallback((
    id: string, 
    data?: unknown, 
    variant: ModalVariant = 'default'
  ) => {
    setModals(prev => new Map(prev).set(id, {
      isOpen: true,
      data,
      variant,
    }));
  }, []);

  const closeModal = useCallback((id: string) => {
    setModals(prev => {
      const newModals = new Map(prev);
      const modal = newModals.get(id);
      if (modal) {
        newModals.set(id, { ...modal, isOpen: false });
      }
      return newModals;
    });
  }, []);

  const closeAllModals = useCallback(() => {
    setModals(prev => {
      const newModals = new Map();
      prev.forEach((modal, id) => {
        newModals.set(id, { ...modal, isOpen: false });
      });
      return newModals;
    });
  }, []);

  const isModalOpen = useCallback((id: string): boolean => {
    return modals.get(id)?.isOpen ?? false;
  }, [modals]);

  const getModalData = useCallback((id: string): unknown => {
    return modals.get(id)?.data;
  }, [modals]);

  const hasOpenModals = Array.from(modals.values()).some(modal => modal.isOpen);

  // Handle escape key for modal stack
  useEffect(() => {
    if (!hasOpenModals) return;

    const handleEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        // Close the most recently opened modal
        const openModalIds = Array.from(modals.entries())
          .filter(([, modal]) => modal.isOpen)
          .map(([id]) => id);
        
        if (openModalIds.length > 0) {
          closeModal(openModalIds[openModalIds.length - 1]);
        }
      }
    };

    document.addEventListener('keydown', handleEscape);
    return () => document.removeEventListener('keydown', handleEscape);
  }, [hasOpenModals, modals, closeModal]);

  // Manage body scroll for modal stack
  useEffect(() => {
    if (hasOpenModals) {
      document.body.style.overflow = 'hidden';
      return () => {
        document.body.style.overflow = 'unset';
      };
    }
  }, [hasOpenModals]);

  return {
    modals,
    openModal,
    closeModal,
    closeAllModals,
    isModalOpen,
    getModalData,
    hasOpenModals,
  };
}
