interface Waiter<T> {
  readonly predicate: (message: T) => boolean;
  readonly resolve: (message: T) => void;
  readonly reject: (error: Error) => void;
  readonly timer: ReturnType<typeof setTimeout> | undefined;
}

/**
 * Buffret innboks for innkommende protokollmeldinger. Løser en reell
 * kappløpssituasjon: en melding kan ankomme MELLOM at vi abonnerer på
 * transporten og at vi begynner å vente på den. Alt som pushes uten en
 * ventende mottaker legges i kø, slik at et senere `next()` finner den.
 */
export class MessageInbox<T> {
  private readonly queue: T[] = [];
  private readonly waiters: Waiter<T>[] = [];
  private closedError: Error | undefined;

  push(message: T): void {
    const index = this.waiters.findIndex((waiter) => waiter.predicate(message));
    if (index >= 0) {
      const [waiter] = this.waiters.splice(index, 1);
      if (waiter) {
        if (waiter.timer !== undefined) clearTimeout(waiter.timer);
        waiter.resolve(message);
      }
      return;
    }
    this.queue.push(message);
  }

  /** Venter på første melding som matcher `predicate`. Køede meldinger sjekkes først. */
  next(predicate: (message: T) => boolean, timeoutMs: number, timeoutError: () => Error): Promise<T> {
    if (this.closedError) return Promise.reject(this.closedError);

    const queuedIndex = this.queue.findIndex(predicate);
    if (queuedIndex >= 0) {
      const [message] = this.queue.splice(queuedIndex, 1);
      // findIndex traff, så elementet finnes — men noUncheckedIndexedAccess
      // ser det ikke, og en stille `as` ville skjult en reell feil her.
      if (message !== undefined) return Promise.resolve(message);
    }

    return new Promise<T>((resolve, reject) => {
      const timer =
        timeoutMs > 0
          ? setTimeout(() => {
              const index = this.waiters.findIndex((waiter) => waiter.resolve === resolve);
              if (index >= 0) this.waiters.splice(index, 1);
              reject(timeoutError());
            }, timeoutMs)
          : undefined;
      this.waiters.push({ predicate, resolve, reject, timer });
    });
  }

  /** Avbryter alle ventende mottakere. Etterfølgende `next()` avvises umiddelbart. */
  close(error: Error): void {
    this.closedError = error;
    const pending = this.waiters.splice(0, this.waiters.length);
    for (const waiter of pending) {
      if (waiter.timer !== undefined) clearTimeout(waiter.timer);
      waiter.reject(error);
    }
    this.queue.length = 0;
  }
}
