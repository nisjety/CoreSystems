'use client';

import type { AnchorHTMLAttributes, ButtonHTMLAttributes, ReactNode } from 'react';

type AnimatedArrowBaseProps = {
    children: ReactNode;
    className?: string;
    motionClassName?: string;
    iconClassName?: string;
};

type AnimatedArrowLinkProps = AnimatedArrowBaseProps & AnchorHTMLAttributes<HTMLAnchorElement>;
type AnimatedArrowButtonProps = AnimatedArrowBaseProps & ButtonHTMLAttributes<HTMLButtonElement>;

function joinClasses(...classNames: Array<string | undefined>) {
    return classNames.filter(Boolean).join(' ');
}

function ArrowLineIcon({ className = 'h-2.5 w-5.5' }: { className?: string }) {
    return (
        <svg
            xmlns="http://www.w3.org/2000/svg"
            viewBox="0 0 22.35 7.16"
            className={className}
            fill="currentColor"
            aria-hidden="true"
        >
            <path d="m18.77 0 3.58 3.58c-.76 0-1.52-.29-2.1-.87l-2.1-2.1.62-.61zm-.61 6.54 2.1-2.1c.58-.58 1.34-.87 2.1-.87l-3.58 3.58-.62-.61zm.28-2.53v-.87H0V4h18.44z" />
        </svg>
    );
}

function AnimatedArrowContent({ children, motionClassName, iconClassName }: Pick<AnimatedArrowBaseProps, 'children' | 'motionClassName' | 'iconClassName'>) {
    return (
        <span
            className={joinClasses(
                'relative flex items-center gap-3 whitespace-nowrap transition-transform duration-500 ease-[cubic-bezier(0.22,1,0.36,1)] -translate-x-8 md:-translate-x-7 group-hover/cta:translate-x-0 group-focus-visible/cta:translate-x-0',
                motionClassName
            )}
        >
            <ArrowLineIcon className={iconClassName} />
            <span className="whitespace-nowrap">{children}</span>
            <ArrowLineIcon className={joinClasses('absolute left-full ml-3', iconClassName)} />
        </span>
    );
}

export function AnimatedArrowLink({ children, className, motionClassName, iconClassName, ...props }: AnimatedArrowLinkProps) {
    return (
        <a
            {...props}
            className={joinClasses(
                'group/cta inline-flex w-fit items-center overflow-hidden py-1 focus-visible:outline-none',
                className
            )}
        >
            <AnimatedArrowContent motionClassName={motionClassName} iconClassName={iconClassName}>
                {children}
            </AnimatedArrowContent>
        </a>
    );
}

export function AnimatedArrowButton({ children, className, motionClassName, iconClassName, type = 'button', ...props }: AnimatedArrowButtonProps) {
    return (
        <button
            {...props}
            type={type}
            className={joinClasses(
                'group/cta inline-flex w-fit items-center overflow-hidden py-1 focus-visible:outline-none',
                className
            )}
        >
            <AnimatedArrowContent motionClassName={motionClassName} iconClassName={iconClassName}>
                {children}
            </AnimatedArrowContent>
        </button>
    );
}
