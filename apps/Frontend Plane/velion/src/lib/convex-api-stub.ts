export const api = new Proxy({}, {
    get: function (target, prop, receiver) {
        if (typeof prop === "string") {
            return new Proxy({}, {
                get: function (innerTarget, innerProp) {
                    if (typeof innerProp === "string") {
                        return `${prop}:${innerProp}`;
                    }
                    return Reflect.get(innerTarget, innerProp);
                }
            });
        }
        return Reflect.get(target, prop, receiver);
    }
}) as any;
