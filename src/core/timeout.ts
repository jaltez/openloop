export class RunTimeoutError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "RunTimeoutError";
  }
}

export async function withTimeout<T>(promise: Promise<T>, timeoutMs: number | undefined, message: string): Promise<T> {
  if (timeoutMs === undefined) {
    return promise;
  }

  if (timeoutMs <= 0) {
    throw new RunTimeoutError(message);
  }

  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => {
      reject(new RunTimeoutError(message));
    }, timeoutMs);

    promise.then(
      (value) => {
        clearTimeout(timer);
        resolve(value);
      },
      (error) => {
        clearTimeout(timer);
        reject(error);
      },
    );
  });
}