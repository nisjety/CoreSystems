import http from 'node:http';
import * as number from 'lib0/number';

const CALLBACK_URL = process.env.CALLBACK_URL ? new URL(process.env.CALLBACK_URL) : null;
const CALLBACK_TIMEOUT = number.parseInt(process.env.CALLBACK_TIMEOUT || '5000');
const CALLBACK_OBJECTS = process.env.CALLBACK_OBJECTS
  ? JSON.parse(process.env.CALLBACK_OBJECTS)
  : {};

export const isCallbackSet = !!CALLBACK_URL;

export const callbackHandler = (doc) => {
  const room = doc.name;
  const dataToSend = {
    room,
    data: {},
  };

  const sharedObjectList = Object.keys(CALLBACK_OBJECTS);
  sharedObjectList.forEach((sharedObjectName) => {
    const sharedObjectType = CALLBACK_OBJECTS[sharedObjectName];
    dataToSend.data[sharedObjectName] = {
      type: sharedObjectType,
      content: getContent(sharedObjectName, sharedObjectType, doc).toJSON(),
    };
  });

  if (CALLBACK_URL) {
    callbackRequest(CALLBACK_URL, CALLBACK_TIMEOUT, dataToSend);
  }
};

const callbackRequest = (url, timeout, data) => {
  const payload = JSON.stringify(data);
  const options = {
    hostname: url.hostname,
    port: url.port,
    path: url.pathname,
    timeout,
    method: 'POST',
    headers: {
      'Content-Length': Buffer.byteLength(payload),
      'Content-Type': 'application/json',
    },
  };

  const request = http.request(options);
  request.on('timeout', () => {
    console.warn('Callback request timed out.');
    request.abort();
  });
  request.on('error', (error) => {
    console.error('Callback request error.', error);
    request.abort();
  });
  request.write(payload);
  request.end();
};

const getContent = (objectName, objectType, doc) => {
  switch (objectType) {
    case 'Array':
      return doc.getArray(objectName);
    case 'Map':
      return doc.getMap(objectName);
    case 'Text':
      return doc.getText(objectName);
    case 'XmlFragment':
      return doc.getXmlFragment(objectName);
    case 'XmlElement':
      return doc.getXmlElement(objectName);
    default:
      return {};
  }
};